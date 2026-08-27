//! Typed application boundary between the privileged desktop backend and the WebView.

mod profiles;
mod top_gear;

pub use profiles::{
    ArmoryRefreshStatus, CharacterProfileView, DisabledArmoryProvider, ProfileInputSource,
};
pub use top_gear::{
    PreparedTopGear, StartedTopGear, TopGearRequest, TopGearResultView, TopGearSessionView,
};

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use simc_adapter::verify_artifact_directory;
use simshredder_core::{InputFormat, PreparedRun};
use simshredder_domain::{NormalizedQuickResult, Role};
use simshredder_job_runner::{
    CancellationToken, CpuPreset, DispatchResult, PersistentQueue, apply_cpu_preset,
};
use simshredder_runtime_manager::RuntimeDoctor;
use simshredder_storage::{AttemptSnapshot, LogStream, PersistentState};

const MAX_TEXT_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Quick Sim request is invalid: {0}")]
    InvalidRequest(String),
    #[error("headless preparation failed: {0}")]
    Core(#[from] simshredder_core::Error),
    #[error("persistent runner failed: {0}")]
    Runner(#[from] simshredder_job_runner::Error),
    #[error("storage failed: {0}")]
    Storage(#[from] simshredder_storage::Error),
    #[error("artifact verification failed: {0}")]
    Adapter(#[from] simc_adapter::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Top Gear failed: {0}")]
    TopGear(#[from] simshredder_top_gear::Error),
    #[error("character profile failed: {0}")]
    CharacterProfile(String),
    #[error("Armory reload is unavailable: {0}")]
    ArmoryUnavailable(String),
    #[error("job {0} has no verified successful artifact")]
    ResultUnavailable(i64),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceFormat {
    AddonExport,
    SimcFile,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CpuChoice {
    Efficient,
    Balanced,
    Maximum,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct QuickSimRequest {
    pub source: String,
    pub format: SourceFormat,
    pub iterations: u32,
    pub fixed_time: bool,
    pub max_time_seconds: u32,
    pub vary_combat_length: f64,
    pub desired_targets: u16,
    pub fight_style: String,
    pub cpu_preset: CpuChoice,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub name: String,
    pub class: String,
    pub specialization: String,
    pub race: String,
    pub role: String,
    pub level: u16,
    pub equipped_items: usize,
    pub bag_items: usize,
    pub talents: Vec<TalentConfiguration>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentConfiguration {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedQuickSim {
    pub profile: ProfileSummary,
    pub generated_input: String,
    pub threads: u16,
    pub profileset_work_threads: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub id: i64,
    pub sequence: u32,
    pub state: String,
    pub failure: Option<String>,
    pub cache_hit: bool,
    pub stdout_log_truncated: bool,
    pub stderr_log_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: i64,
    pub state: String,
    pub cancel_requested: bool,
    pub failure: Option<String>,
    pub succeeded_batches: u32,
    pub pending_batches: u32,
    pub attempts: Vec<AttemptView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickResultView {
    pub job_id: i64,
    pub result: NormalizedQuickResult,
    pub generated_input: String,
    pub raw_json: String,
    pub raw_html: String,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub artifact_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportView {
    pub directory: PathBuf,
    pub file_count: usize,
}

#[derive(Clone, Debug)]
pub struct DesktopService {
    database_path: PathBuf,
    run_root: PathBuf,
    profile_catalog_path: PathBuf,
}

impl DesktopService {
    pub fn open(app_data_root: impl Into<PathBuf>) -> Result<Self> {
        let root = app_data_root.into();
        fs::create_dir_all(&root)?;
        let service = Self {
            database_path: root.join("jobs.sqlite3"),
            run_root: root.join("runs"),
            profile_catalog_path: root.join("character-profiles.json"),
        };
        let _ = service.queue()?;
        Ok(service)
    }

    pub fn prepare(&self, request: &QuickSimRequest) -> Result<PreparedQuickSim> {
        let (prepared, plan) = prepare_request(request)?;
        Ok(PreparedQuickSim {
            profile: summarize(&prepared),
            generated_input: String::from_utf8(prepared.generated_bytes)
                .map_err(|_| Error::InvalidRequest("generated input is not UTF-8".into()))?,
            threads: plan.threads,
            profileset_work_threads: plan.profileset_work_threads,
        })
    }

    pub fn enqueue(
        &self,
        request: &QuickSimRequest,
        runtime: &RuntimeDoctor,
    ) -> Result<(i64, CancellationToken)> {
        let (prepared, _) = prepare_request(request)?;
        let mut queue = self.queue()?;
        let enqueued = simshredder_core::enqueue_persistent(
            &mut queue,
            &prepared,
            &runtime.executable,
            &runtime.record.build,
            cpu_preset(request.cpu_preset),
            Duration::from_secs(30 * 60),
        )?;
        Ok((
            enqueued.job_id,
            queue.cancel_handle(enqueued.job_id).token(),
        ))
    }

    pub fn run_next(&self, token: CancellationToken) -> Result<DispatchResult> {
        Ok(self.queue()?.run_next(token)?)
    }

    pub fn recover(&self) -> Result<Vec<i64>> {
        Ok(self.queue()?.recover_and_resume()?.interrupted_jobs)
    }

    pub fn cancel(&self, job_id: i64, token: &CancellationToken) -> Result<()> {
        let queue = self.queue()?;
        let handle = queue.cancel_handle(job_id);
        handle.cancel()?;
        token.cancel();
        Ok(())
    }

    pub fn retry(&self, job_id: i64) -> Result<()> {
        self.queue()?.database_mut().retry_job(job_id)?;
        Ok(())
    }

    pub fn job(&self, job_id: i64) -> Result<JobView> {
        let queue = self.queue()?;
        let snapshot = queue.database().job(job_id)?;
        let attempts = queue
            .database()
            .attempts_for_job(job_id)?
            .into_iter()
            .map(attempt_view)
            .collect();
        Ok(JobView {
            id: snapshot.id,
            state: snapshot.state.as_str().into(),
            cancel_requested: snapshot.cancel_requested,
            failure: snapshot.failure,
            succeeded_batches: snapshot.succeeded_batches,
            pending_batches: snapshot.pending_batches,
            attempts,
        })
    }

    pub fn result(&self, job_id: i64) -> Result<QuickResultView> {
        let queue = self.queue()?;
        let audit = queue.audit_job_artifacts(job_id)?;
        if audit.iter().any(|entry| !entry.valid) {
            return Err(Error::ResultUnavailable(job_id));
        }
        let artifacts = queue.database().batch_artifacts(job_id)?;
        let artifact_directory = artifacts
            .into_iter()
            .find(|entry| entry.state == PersistentState::Succeeded)
            .and_then(|entry| entry.artifact_directory)
            .ok_or(Error::ResultUnavailable(job_id))?;
        let _ = verify_artifact_directory(&artifact_directory)?;
        let attempts = queue.database().attempts_for_job(job_id)?;
        let attempt = attempts
            .iter()
            .rev()
            .find(|entry| entry.state == PersistentState::Succeeded)
            .ok_or(Error::ResultUnavailable(job_id))?;
        let result: NormalizedQuickResult =
            serde_json::from_slice(&read_regular(&artifact_directory.join("normalized.json"))?)?;
        Ok(QuickResultView {
            job_id,
            result,
            generated_input: read_text(&artifact_directory.join("generated.simc"))?,
            raw_json: read_text(&artifact_directory.join("result.json"))?,
            raw_html: read_text(&artifact_directory.join("report.html"))?,
            stdout: String::from_utf8_lossy(
                &queue.database().read_log(attempt.id, LogStream::Stdout)?,
            )
            .into_owned(),
            stderr: String::from_utf8_lossy(
                &queue.database().read_log(attempt.id, LogStream::Stderr)?,
            )
            .into_owned(),
            stdout_truncated: attempt.stdout_log_truncated,
            stderr_truncated: attempt.stderr_log_truncated,
            artifact_directory,
        })
    }

    pub fn export(&self, job_id: i64, destination_root: &Path) -> Result<ExportView> {
        let result = self.result(job_id)?;
        fs::create_dir_all(destination_root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidRequest("system clock is before the Unix epoch".into()))?
            .as_millis();
        let destination = destination_root.join(format!("quick-sim-{job_id}-{timestamp}"));
        let staging = destination_root.join(format!(
            ".quick-sim-{job_id}-{timestamp}-{}.staging",
            std::process::id()
        ));
        fs::create_dir(&staging)?;
        protect_directory(&staging)?;
        let files = [
            ("generated.simc", result.generated_input.as_bytes()),
            ("result.json", result.raw_json.as_bytes()),
            ("report.html", result.raw_html.as_bytes()),
            ("stdout.log", result.stdout.as_bytes()),
            ("stderr.log", result.stderr.as_bytes()),
        ];
        for (name, bytes) in files {
            let path = staging.join(name);
            fs::write(&path, bytes)?;
            protect_file(&path)?;
        }
        fs::rename(&staging, &destination)?;
        Ok(ExportView {
            directory: destination,
            file_count: files.len(),
        })
    }

    fn queue(&self) -> Result<PersistentQueue> {
        Ok(PersistentQueue::open(&self.database_path, &self.run_root)?)
    }
}

fn prepare_request(
    request: &QuickSimRequest,
) -> Result<(PreparedRun, simshredder_job_runner::CpuPlan)> {
    validate_request(request)?;
    let format = match request.format {
        SourceFormat::AddonExport => InputFormat::AddonExport,
        SourceFormat::SimcFile => InputFormat::SimcFile,
    };
    let mut prepared = simshredder_core::prepare(&request.source, format)?;
    prepared.profile.simulation.iterations = request.iterations;
    prepared.profile.simulation.fixed_time = request.fixed_time;
    prepared.profile.simulation.max_time_seconds = request.max_time_seconds;
    prepared.profile.simulation.vary_combat_length = request.vary_combat_length;
    prepared.profile.simulation.desired_targets = request.desired_targets;
    prepared.profile.simulation.fight_style = request.fight_style.clone();
    let logical_cpus = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1);
    let plan = apply_cpu_preset(
        &mut prepared.profile,
        cpu_preset(request.cpu_preset),
        logical_cpus,
    );
    Ok((
        simshredder_core::prepare_profile(&prepared.source_bytes, prepared.profile)?,
        plan,
    ))
}

fn validate_request(request: &QuickSimRequest) -> Result<()> {
    if request.source.is_empty() {
        return Err(Error::InvalidRequest("profile source is empty".into()));
    }
    if !(100..=10_000_000).contains(&request.iterations) {
        return Err(Error::InvalidRequest(
            "iterations must be between 100 and 10,000,000".into(),
        ));
    }
    if !(30..=3_600).contains(&request.max_time_seconds) {
        return Err(Error::InvalidRequest(
            "maximum time must be between 30 and 3,600 seconds".into(),
        ));
    }
    if !(1..=20).contains(&request.desired_targets) {
        return Err(Error::InvalidRequest(
            "target count must be between 1 and 20".into(),
        ));
    }
    if !(0.0..=0.5).contains(&request.vary_combat_length) {
        return Err(Error::InvalidRequest(
            "combat length variance must be between 0 and 0.5".into(),
        ));
    }
    if !matches!(
        request.fight_style.as_str(),
        "Patchwerk" | "DungeonSlice" | "HecticAddCleave" | "LightMovement"
    ) {
        return Err(Error::InvalidRequest(
            "fight style is not in the supported GUI allowlist".into(),
        ));
    }
    Ok(())
}

const fn cpu_preset(choice: CpuChoice) -> CpuPreset {
    match choice {
        CpuChoice::Efficient => CpuPreset::Efficient,
        CpuChoice::Balanced => CpuPreset::Balanced,
        CpuChoice::Maximum => CpuPreset::Maximum,
    }
}

fn summarize(prepared: &PreparedRun) -> ProfileSummary {
    let profile = &prepared.profile;
    ProfileSummary {
        name: profile.name.clone(),
        class: profile.class.simc_token().into(),
        specialization: profile.specialization.clone(),
        race: profile.race.clone(),
        role: match profile.role {
            Role::Attack => "attack",
            Role::Heal => "heal",
            Role::Tank => "tank",
        }
        .into(),
        level: profile.level,
        equipped_items: profile.equipped.len(),
        bag_items: profile.bag_items.len(),
        talents: profile
            .talents
            .iter()
            .map(|(name, value)| TalentConfiguration {
                name: name.clone(),
                value: value.clone(),
            })
            .collect(),
        warnings: Vec::new(),
    }
}

fn attempt_view(attempt: AttemptSnapshot) -> AttemptView {
    AttemptView {
        id: attempt.id,
        sequence: attempt.sequence,
        state: attempt.state.as_str().into(),
        failure: attempt.failure,
        cache_hit: attempt.cache_hit,
        stdout_log_truncated: attempt.stdout_log_truncated,
        stderr_log_truncated: attempt.stderr_log_truncated,
    }
}

fn read_regular(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_TEXT_ARTIFACT_BYTES
    {
        return Err(Error::InvalidRequest(format!(
            "artifact {} is not a bounded regular file",
            path.display()
        )));
    }
    Ok(fs::read(path)?)
}

fn read_text(path: &Path) -> Result<String> {
    String::from_utf8(read_regular(path)?)
        .map_err(|_| Error::InvalidRequest(format!("artifact {} is not UTF-8", path.display())))
}

#[cfg(unix)]
fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn protect_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn protect_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> QuickSimRequest {
        QuickSimRequest {
            source:
                "warrior=Core\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nload_default_gear=1\n"
                    .into(),
            format: SourceFormat::SimcFile,
            iterations: 12_345,
            fixed_time: false,
            max_time_seconds: 240,
            vary_combat_length: 0.1,
            desired_targets: 3,
            fight_style: "Patchwerk".into(),
            cpu_preset: CpuChoice::Balanced,
        }
    }

    #[test]
    fn preview_matches_every_gui_simulation_choice() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let preview = service.prepare(&request()).unwrap();
        assert!(preview.generated_input.contains("iterations=12345\n"));
        assert!(preview.generated_input.contains("max_time=240\n"));
        assert!(preview.generated_input.contains("vary_combat_length=0.1\n"));
        assert!(preview.generated_input.contains("desired_targets=3\n"));
        assert!(preview.generated_input.contains("fight_style=Patchwerk\n"));
        assert!(
            preview
                .generated_input
                .contains(&format!("threads={}\n", preview.threads))
        );
        assert_eq!(preview.profile.name, "Core");
    }

    #[test]
    fn rejects_out_of_range_or_unlisted_gui_options() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut invalid = request();
        invalid.iterations = 99;
        assert!(matches!(
            service.prepare(&invalid),
            Err(Error::InvalidRequest(_))
        ));
        invalid = request();
        invalid.fight_style = "include=/tmp/evil".into();
        assert!(matches!(
            service.prepare(&invalid),
            Err(Error::InvalidRequest(_))
        ));
    }
}

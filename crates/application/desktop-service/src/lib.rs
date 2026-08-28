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
use simshredder_domain::{ActionDirective, NormalizedQuickResult, Role};
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrecisionChoice {
    Smart,
    #[default]
    Fixed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct AnalysisOptions {
    pub precision: PrecisionChoice,
    pub target_error: f64,
    pub target_level: u16,
    pub target_race: String,
    pub world_lag_ms: u16,
    pub world_lag_stddev_ms: u16,
    pub player_skill: f64,
    pub seed: u64,
    pub optimal_raid: bool,
    pub bloodlust: bool,
    pub bloodlust_time: i32,
    pub bloodlust_percent: u8,
    pub consumables: bool,
    pub raid_buffs: RaidBuffOptions,
    pub consumable_options: ConsumableOptions,
    pub report_details: bool,
    pub report_pets_separately: bool,
    pub custom_apl: String,
    pub custom_options: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RaidBuffOptions {
    pub arcane_intellect: bool,
    pub battle_shout: bool,
    pub mark_of_the_wild: bool,
    pub power_word_fortitude: bool,
    pub chaos_brand: bool,
    pub mystic_touch: bool,
    pub windfury_totem: bool,
    pub hunters_mark: bool,
    pub bleeding: bool,
}

impl Default for RaidBuffOptions {
    fn default() -> Self {
        Self {
            arcane_intellect: true,
            battle_shout: true,
            mark_of_the_wild: true,
            power_word_fortitude: true,
            chaos_brand: true,
            mystic_touch: true,
            windfury_totem: true,
            hunters_mark: true,
            bleeding: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ConsumableOptions {
    pub flask: bool,
    pub food: bool,
    pub augmentation: bool,
    pub potion: bool,
    pub temporary_enchant: bool,
}

impl Default for ConsumableOptions {
    fn default() -> Self {
        Self {
            flask: true,
            food: true,
            augmentation: true,
            potion: true,
            temporary_enchant: true,
        }
    }
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            precision: PrecisionChoice::Fixed,
            target_error: 0.2,
            target_level: 93,
            target_race: "humanoid".into(),
            world_lag_ms: 50,
            world_lag_stddev_ms: 5,
            player_skill: 1.0,
            seed: 1,
            optimal_raid: true,
            bloodlust: true,
            bloodlust_time: 0,
            bloodlust_percent: 0,
            consumables: true,
            raid_buffs: RaidBuffOptions::default(),
            consumable_options: ConsumableOptions::default(),
            report_details: true,
            report_pets_separately: false,
            custom_apl: String::new(),
            custom_options: String::new(),
        }
    }
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
    #[serde(default)]
    pub analysis: AnalysisOptions,
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
    pub input_compatibility: InputCompatibilityView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputCompatibilityView {
    pub supported_editable: usize,
    pub preserved_not_editable: usize,
    pub execution_blocked: usize,
    pub diagnostics: Vec<InputCompatibilityDiagnosticView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputCompatibilityDiagnosticView {
    pub line: usize,
    pub key: Option<String>,
    pub category: String,
    pub reason: String,
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

    pub fn recent_jobs(&self) -> Result<Vec<JobView>> {
        let queue = self.queue()?;
        queue
            .database()
            .recent_jobs(200)?
            .into_iter()
            .map(|snapshot| {
                let attempts = queue
                    .database()
                    .attempts_for_job(snapshot.id)?
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
            })
            .collect()
    }

    pub fn delete_job(&self, job_id: i64) -> Result<()> {
        self.delete_jobs(&[job_id])
    }

    fn delete_jobs(&self, job_ids: &[i64]) -> Result<()> {
        let mut queue = self.queue()?;
        let externally_referenced = queue
            .database()
            .artifact_directories_excluding_jobs(job_ids)?;
        let mut planned = Vec::new();
        for job_id in job_ids {
            let source = self.run_root.join(format!("job-{job_id}"));
            match fs::symlink_metadata(&source) {
                Ok(metadata) => {
                    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                        return Err(Error::InvalidRequest(format!(
                            "job {job_id} artifact root is not a regular directory"
                        )));
                    }
                    if externally_referenced
                        .iter()
                        .any(|path| path.starts_with(&source))
                    {
                        continue;
                    }
                    let tombstone = self
                        .run_root
                        .join(format!(".deleted-job-{job_id}-{}", std::process::id()));
                    if fs::symlink_metadata(&tombstone).is_ok() {
                        return Err(Error::InvalidRequest(format!(
                            "job {job_id} deletion is already staged"
                        )));
                    }
                    planned.push((source, tombstone));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        let mut moved = Vec::new();
        for (source, tombstone) in planned {
            if let Err(error) = fs::rename(&source, &tombstone) {
                for (restored_source, restored_tombstone) in moved.iter().rev() {
                    let _ = fs::rename(restored_tombstone, restored_source);
                }
                return Err(error.into());
            }
            moved.push((source, tombstone));
        }

        if let Err(error) = queue.database_mut().delete_terminal_jobs(job_ids) {
            for (source, tombstone) in moved.iter().rev() {
                let _ = fs::rename(tombstone, source);
            }
            return Err(error.into());
        }
        for (_, tombstone) in moved {
            let _ = fs::remove_dir_all(tombstone);
        }
        Ok(())
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
    apply_analysis_options(&mut prepared.profile, &request.analysis)?;
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
        "Patchwerk"
            | "CastingPatchwerk"
            | "DungeonSlice"
            | "HecticAddCleave"
            | "LightMovement"
            | "HeavyMovement"
            | "HelterSkelter"
            | "CleaveAdd"
            | "Beastlord"
    ) {
        return Err(Error::InvalidRequest(
            "fight style is not in the supported GUI allowlist".into(),
        ));
    }
    validate_analysis_options(&request.analysis)?;
    Ok(())
}

fn validate_analysis_options(options: &AnalysisOptions) -> Result<()> {
    if !options.target_error.is_finite() || !(0.01..=5.0).contains(&options.target_error) {
        return Err(Error::InvalidRequest(
            "target error must be between 0.01 and 5 percent".into(),
        ));
    }
    if !(1..=100).contains(&options.target_level)
        || !matches!(
            options.target_race.as_str(),
            "humanoid"
                | "aberration"
                | "beast"
                | "demon"
                | "dragonkin"
                | "elemental"
                | "giant"
                | "mechanical"
                | "undead"
                | "not_specified"
        )
        || options.world_lag_ms > 2_000
        || options.world_lag_stddev_ms > 1_000
        || !options.player_skill.is_finite()
        || !(0.1..=1.0).contains(&options.player_skill)
        || options.seed == 0
        || !(-3_600..=3_600).contains(&options.bloodlust_time)
        || options.bloodlust_percent > 100
        || options.custom_apl.len() > 256 * 1024
        || options.custom_options.len() > 64 * 1024
    {
        return Err(Error::InvalidRequest(
            "one or more advanced analysis options are outside the supported range".into(),
        ));
    }
    Ok(())
}

fn apply_analysis_options(
    profile: &mut simshredder_domain::Profile,
    options: &AnalysisOptions,
) -> Result<()> {
    profile.scalar_options.insert(
        "target_error".into(),
        match options.precision {
            PrecisionChoice::Smart => options.target_error.to_string(),
            PrecisionChoice::Fixed => "0".into(),
        },
    );
    for (key, value) in [
        ("target_level", options.target_level.to_string()),
        ("target_race", options.target_race.clone()),
        ("default_world_lag", format_seconds(options.world_lag_ms)),
        (
            "default_world_lag_stddev",
            format_seconds(options.world_lag_stddev_ms),
        ),
        ("skill", options.player_skill.to_string()),
        ("optimal_raid", u8::from(options.optimal_raid).to_string()),
        (
            "override.bloodlust",
            u8::from(options.bloodlust).to_string(),
        ),
        ("bloodlust_time", options.bloodlust_time.to_string()),
        ("bloodlust_percent", options.bloodlust_percent.to_string()),
        (
            "report_pets_separately",
            u8::from(options.report_pets_separately).to_string(),
        ),
    ] {
        profile.scalar_options.insert(key.into(), value);
    }
    for (key, enabled) in [
        (
            "override.arcane_intellect",
            options.raid_buffs.arcane_intellect,
        ),
        ("override.battle_shout", options.raid_buffs.battle_shout),
        (
            "override.mark_of_the_wild",
            options.raid_buffs.mark_of_the_wild,
        ),
        (
            "override.power_word_fortitude",
            options.raid_buffs.power_word_fortitude,
        ),
        ("override.chaos_brand", options.raid_buffs.chaos_brand),
        ("override.mystic_touch", options.raid_buffs.mystic_touch),
        ("override.windfury_totem", options.raid_buffs.windfury_totem),
        ("override.hunters_mark", options.raid_buffs.hunters_mark),
        ("override.bleeding", options.raid_buffs.bleeding),
    ] {
        profile
            .scalar_options
            .insert(key.into(), u8::from(enabled).to_string());
    }
    profile.simulation.report_details = options.report_details;
    profile.simulation.seed = options.seed;
    for (key, enabled) in [
        ("flask", options.consumable_options.flask),
        ("food", options.consumable_options.food),
        ("augmentation", options.consumable_options.augmentation),
        ("potion", options.consumable_options.potion),
        (
            "temporary_enchant",
            options.consumable_options.temporary_enchant,
        ),
    ] {
        if !options.consumables || !enabled {
            profile.scalar_options.remove(key);
        }
    }
    apply_custom_options(&mut profile.scalar_options, &options.custom_options)?;
    if !options.custom_apl.trim().is_empty() {
        profile.actions = parse_custom_apl(&options.custom_apl)?;
        profile
            .scalar_options
            .insert("default_actions".into(), "0".into());
    }
    Ok(())
}

fn format_seconds(milliseconds: u16) -> String {
    let seconds = f64::from(milliseconds) / 1_000.0;
    format!("{seconds:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn apply_custom_options(
    destination: &mut std::collections::BTreeMap<String, String>,
    source: &str,
) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            Error::InvalidRequest(format!(
                "custom SimC option line {} must use key=value",
                index + 1
            ))
        })?;
        let key = key.trim();
        let value = value.trim();
        if !is_supported_custom_option(key)
            || value.is_empty()
            || value.len() > 4_096
            || value.chars().any(char::is_control)
        {
            return Err(Error::InvalidRequest(format!(
                "custom SimC option line {} is unsupported or unsafe",
                index + 1
            )));
        }
        if !seen.insert(key.to_owned()) {
            return Err(Error::InvalidRequest(format!(
                "custom SimC option duplicates {key}"
            )));
        }
        destination.insert(key.into(), value.into());
    }
    Ok(())
}

fn is_supported_custom_option(key: &str) -> bool {
    matches!(
        key,
        "override.arcane_intellect"
            | "override.battle_shout"
            | "override.mark_of_the_wild"
            | "override.power_word_fortitude"
            | "override.chaos_brand"
            | "override.mystic_touch"
            | "override.windfury_totem"
            | "override.hunters_mark"
            | "override.bleeding"
            | "external_buffs.power_infusion"
            | "external_buffs.blessing_of_summer"
            | "external_buffs.blessing_of_autumn"
            | "external_buffs.blessing_of_winter"
            | "external_buffs.blessing_of_spring"
            | "report_rng"
            | "full_damage_sources_chart"
            | "buff_uptime_timeline"
            | "buff_stack_uptime_timeline"
            | "reaction_time"
            | "queue_lag"
            | "queue_lag_stddev"
            | "gcd_lag"
            | "gcd_lag_stddev"
            | "channel_lag"
            | "channel_lag_stddev"
            | "travel_variance"
            | "enemy_initial_health_percentage"
            | "enemy_death_pct"
    ) || key.starts_with("midnight.")
}

fn parse_custom_apl(source: &str) -> Result<Vec<ActionDirective>> {
    let mut actions = Vec::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            Error::InvalidRequest(format!(
                "custom APL line {} must use actions...=...",
                index + 1
            ))
        })?;
        if !key.starts_with("actions")
            || key.len() > 128
            || !key.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '_' | '.' | '+')
            })
            || value.is_empty()
            || value.len() > 16 * 1024
            || value.chars().any(char::is_control)
        {
            return Err(Error::InvalidRequest(format!(
                "custom APL line {} is unsupported or unsafe",
                index + 1
            )));
        }
        actions.push(ActionDirective {
            key: key.into(),
            value: value.into(),
        });
    }
    if actions.is_empty() {
        return Err(Error::InvalidRequest("custom APL is empty".into()));
    }
    Ok(actions)
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
        input_compatibility: InputCompatibilityView {
            supported_editable: prepared.compatibility.supported_editable,
            preserved_not_editable: prepared.compatibility.preserved_not_editable,
            execution_blocked: prepared.compatibility.execution_blocked,
            diagnostics: prepared
                .compatibility
                .diagnostics
                .iter()
                .map(|entry| InputCompatibilityDiagnosticView {
                    line: entry.line,
                    key: entry.key.clone(),
                    category: match entry.category {
                        profile_parser::CompatibilityCategory::SupportedEditable => {
                            "supportedEditable"
                        }
                        profile_parser::CompatibilityCategory::PreservedNotEditable => {
                            "preservedNotEditable"
                        }
                        profile_parser::CompatibilityCategory::ExecutionBlocked => {
                            "executionBlocked"
                        }
                    }
                    .into(),
                    reason: entry.reason.clone(),
                })
                .collect(),
        },
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
    use simshredder_domain::SourceKind;
    use simshredder_storage::{CpuPreset as StorageCpuPreset, NewBatch, NewJob};

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
            analysis: AnalysisOptions::default(),
        }
    }

    #[test]
    fn deleting_a_terminal_job_removes_its_record_and_unshared_run_directory() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut queue = service.queue().unwrap();
        let database = queue.database_mut();
        let profile_id = database
            .insert_profile(SourceKind::SimcFile, b"source", b"generated", "{}")
            .unwrap();
        let job_id = database
            .enqueue_job(
                &NewJob {
                    profile_id,
                    cpu_preset: StorageCpuPreset::Balanced,
                    executable_path: PathBuf::from("/tmp/simc"),
                    runtime_revision: "revision".into(),
                    executable_sha256: "a".repeat(64),
                    simc_version: "1210-01".into(),
                    game_version: "12.1.0.1".into(),
                    normalized_schema_version: 1,
                    rule_revision: "quick-v1".into(),
                    timeout_millis: 60_000,
                },
                &[NewBatch {
                    source_kind: SourceKind::SimcFile,
                    source_bytes: b"source".to_vec(),
                    generated_bytes: b"generated".to_vec(),
                    generated_sha256: "b".repeat(64),
                    cache_key: "c".repeat(64),
                }],
            )
            .unwrap();
        let attempt = database.claim_next_attempt().unwrap().unwrap();
        let artifact = service
            .run_root
            .join(format!("job-{job_id}/batch-0/attempt-1"));
        fs::create_dir_all(&artifact).unwrap();
        database
            .complete_attempt(attempt.attempt_id, &artifact, &"d".repeat(64), false)
            .unwrap();
        drop(queue);

        service.delete_job(job_id).unwrap();
        assert!(service.recent_jobs().unwrap().is_empty());
        assert!(!service.run_root.join(format!("job-{job_id}")).exists());
    }

    #[test]
    fn preview_matches_every_gui_simulation_choice() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut request = request();
        request.analysis.precision = PrecisionChoice::Smart;
        request.analysis.target_error = 0.15;
        request.analysis.target_level = 92;
        request.analysis.target_race = "demon".into();
        request.analysis.world_lag_ms = 75;
        request.analysis.world_lag_stddev_ms = 10;
        request.analysis.player_skill = 0.95;
        request.analysis.seed = 42;
        request.analysis.optimal_raid = false;
        request.analysis.bloodlust = false;
        request.analysis.raid_buffs.arcane_intellect = false;
        request.analysis.consumable_options.potion = false;
        request.analysis.bloodlust_time = -60;
        request.analysis.bloodlust_percent = 20;
        request.analysis.report_details = false;
        request.analysis.report_pets_separately = true;
        request.analysis.custom_options =
            "external_buffs.power_infusion=0/120\nmidnight.sealed_chaos_urn_dispell=1".into();
        request.analysis.custom_apl = "actions=/auto_attack\nactions+=/bloodthirst".into();
        let preview = service.prepare(&request).unwrap();
        assert!(preview.generated_input.contains("iterations=12345\n"));
        assert!(preview.generated_input.contains("max_time=240\n"));
        assert!(preview.generated_input.contains("vary_combat_length=0.1\n"));
        assert!(preview.generated_input.contains("desired_targets=3\n"));
        assert!(preview.generated_input.contains("fight_style=Patchwerk\n"));
        assert!(preview.generated_input.contains("target_error=0.15\n"));
        assert!(preview.generated_input.contains("target_level=92\n"));
        assert!(preview.generated_input.contains("target_race=demon\n"));
        assert!(
            preview
                .generated_input
                .contains("default_world_lag=0.075\n")
        );
        assert!(preview.generated_input.contains("skill=0.95\n"));
        assert!(preview.generated_input.contains("seed=42\n"));
        assert!(preview.generated_input.contains("optimal_raid=0\n"));
        assert!(preview.generated_input.contains("override.bloodlust=0\n"));
        assert!(
            preview
                .generated_input
                .contains("override.arcane_intellect=0\n")
        );
        assert!(preview.generated_input.contains("bloodlust_time=-60\n"));
        assert!(preview.generated_input.contains("bloodlust_percent=20\n"));
        assert!(preview.generated_input.contains("report_details=0\n"));
        assert!(
            preview
                .generated_input
                .contains("report_pets_separately=1\n")
        );
        assert!(
            preview
                .generated_input
                .contains("external_buffs.power_infusion=0/120\n")
        );
        assert!(
            preview
                .generated_input
                .contains("midnight.sealed_chaos_urn_dispell=1\n")
        );
        assert!(preview.generated_input.contains("actions=/auto_attack\n"));
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
        invalid = request();
        invalid.analysis.custom_options = "output=/tmp/stolen".into();
        assert!(matches!(
            service.prepare(&invalid),
            Err(Error::InvalidRequest(_))
        ));
        invalid = request();
        invalid.analysis.custom_apl = "json2=/tmp/stolen".into();
        assert!(matches!(
            service.prepare(&invalid),
            Err(Error::InvalidRequest(_))
        ));
    }
}

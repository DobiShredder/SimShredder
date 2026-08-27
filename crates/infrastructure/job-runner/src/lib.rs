//! Persistent, crash-recoverable orchestration for SimulationCraft batches.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[cfg(unix)]
use std::fs::File;

use serde::Serialize;
use sha2::{Digest, Sha256};
use simc_adapter::{
    Error as AdapterError, HeadlessRunRequest, LogChunk, NORMALIZED_SCHEMA_VERSION, ProcessControl,
    ProcessStream, run_headless_quick_controlled, sha256_file, validate_supported_binary,
    verify_artifact_directory,
};
use simshredder_domain::{Profile, SourceKind};
use simshredder_storage::{ClaimedAttempt, Database, LogStream, NewBatch, NewJob, PersistentState};
use thiserror::Error;

pub use simshredder_storage::CpuPreset;

pub const EXACT_CACHE_SCHEMA_VERSION: u32 = 1;
pub const QUICK_RULE_REVISION: &str = "quick-v1";

#[derive(Debug, Error)]
pub enum Error {
    #[error("persistent storage failed: {0}")]
    Storage(#[from] simshredder_storage::Error),
    #[error("SimulationCraft adapter failed: {0}")]
    Adapter(#[from] AdapterError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("runner contract failed: {0}")]
    Contract(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CpuPlan {
    pub threads: u16,
    pub profileset_work_threads: u16,
}

pub fn cpu_plan(preset: CpuPreset, logical_cpus: usize) -> CpuPlan {
    let logical = logical_cpus.max(1).min(usize::from(u16::MAX));
    let (threads, work_threads) = match preset {
        CpuPreset::Efficient => ((logical / 4).max(1), (logical / 8).max(1)),
        CpuPreset::Balanced => (logical.min(4), (logical / 4).clamp(1, 2)),
        CpuPreset::Maximum => (logical, (logical / 2).max(1)),
    };
    CpuPlan {
        threads: u16::try_from(threads).unwrap_or(u16::MAX),
        profileset_work_threads: u16::try_from(work_threads).unwrap_or(u16::MAX),
    }
}

pub fn apply_cpu_preset(profile: &mut Profile, preset: CpuPreset, logical_cpus: usize) -> CpuPlan {
    let plan = cpu_plan(preset, logical_cpus);
    profile.simulation.threads = plan.threads;
    plan
}

#[derive(Clone, Debug)]
pub struct BatchInput {
    pub source_kind: SourceKind,
    pub source_bytes: Vec<u8>,
    pub generated_bytes: Vec<u8>,
}

pub struct EnqueueRequest<'a> {
    pub profile: &'a Profile,
    pub batches: Vec<BatchInput>,
    pub executable: &'a Path,
    pub expected_revision: &'a str,
    pub cpu_preset: CpuPreset,
    pub timeout: Duration,
    pub rule_revision: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnqueuedJob {
    pub job_id: i64,
    pub cache_keys: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionFailureKind {
    Failed,
    Canceled,
}

#[derive(Clone, Debug)]
pub struct ExecutionFailure {
    pub kind: ExecutionFailureKind,
    pub message: String,
    pub artifacts: Option<PathBuf>,
}

type LogObserver = dyn Fn(ProcessStream, &[u8]) + Send + Sync;

#[derive(Clone)]
pub struct ExecutionControl {
    cancel: CancellationToken,
    on_log: Arc<LogObserver>,
}

impl ExecutionControl {
    pub fn is_canceled(&self) -> bool {
        self.cancel.is_canceled()
    }

    pub fn emit_log(&self, stream: ProcessStream, bytes: &[u8]) {
        (self.on_log)(stream, bytes);
    }
}

pub trait BatchExecutor {
    fn execute(
        &self,
        attempt: &ClaimedAttempt,
        output_directory: &Path,
        control: ExecutionControl,
    ) -> std::result::Result<PathBuf, ExecutionFailure>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SimcBatchExecutor;

impl BatchExecutor for SimcBatchExecutor {
    fn execute(
        &self,
        attempt: &ClaimedAttempt,
        output_directory: &Path,
        control: ExecutionControl,
    ) -> std::result::Result<PathBuf, ExecutionFailure> {
        let observer_control = control.clone();
        let process_control = ProcessControl::new(
            Some(control.cancel.flag.clone()),
            Some(Arc::new(move |chunk: LogChunk| {
                observer_control.emit_log(chunk.stream, &chunk.bytes);
            })),
        );
        let result = run_headless_quick_controlled(
            HeadlessRunRequest {
                executable: &attempt.executable_path,
                expected_revision: &attempt.runtime_revision,
                source_kind: attempt.source_kind,
                source_bytes: &attempt.source_bytes,
                generated_bytes: &attempt.generated_bytes,
                output_directory,
                timeout: Duration::from_millis(attempt.timeout_millis),
            },
            process_control,
        );
        match result {
            Ok(result) => Ok(result.directory),
            Err(AdapterError::ExecutionCanceled { artifacts }) => Err(ExecutionFailure {
                kind: ExecutionFailureKind::Canceled,
                message: "SimulationCraft execution was canceled".into(),
                artifacts: Some(artifacts),
            }),
            Err(AdapterError::ExecutionFailed { status, artifacts }) => Err(ExecutionFailure {
                kind: ExecutionFailureKind::Failed,
                message: status,
                artifacts: Some(artifacts),
            }),
            Err(AdapterError::ResultRejected { reason, artifacts }) => Err(ExecutionFailure {
                kind: ExecutionFailureKind::Failed,
                message: reason,
                artifacts: Some(artifacts),
            }),
            Err(error) => Err(ExecutionFailure {
                kind: if control.is_canceled() {
                    ExecutionFailureKind::Canceled
                } else {
                    ExecutionFailureKind::Failed
                },
                message: error.to_string(),
                artifacts: None,
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }

    pub fn is_canceled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct CancelHandle {
    database_path: PathBuf,
    job_id: i64,
    token: CancellationToken,
}

impl CancelHandle {
    pub fn cancel(&self) -> Result<()> {
        let mut database = Database::open(&self.database_path)?;
        database.request_cancel(self.job_id)?;
        self.token.cancel();
        Ok(())
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DispatchResult {
    Idle,
    Executed { job_id: i64, batch_ordinal: u32 },
    CacheHit { job_id: i64, batch_ordinal: u32 },
    Failed { job_id: i64, batch_ordinal: u32 },
    Canceled { job_id: i64, batch_ordinal: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryReport {
    pub interrupted_jobs: Vec<i64>,
    pub partial_artifacts: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactAuditEntry {
    pub batch_ordinal: u32,
    pub valid: bool,
    pub diagnostic: String,
}

pub struct PersistentQueue {
    database: Database,
    database_path: PathBuf,
    run_root: PathBuf,
}

impl PersistentQueue {
    pub fn open(database_path: &Path, run_root: &Path) -> Result<Self> {
        fs::create_dir_all(run_root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(run_root, fs::Permissions::from_mode(0o700))?;
        }
        let run_root = fs::canonicalize(run_root)?;
        let database = Database::open(database_path)?;
        let database_path = fs::canonicalize(database_path)?;
        Ok(Self {
            database,
            database_path,
            run_root,
        })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn database_mut(&mut self) -> &mut Database {
        &mut self.database
    }

    pub fn enqueue(&mut self, request: EnqueueRequest<'_>) -> Result<EnqueuedJob> {
        if request.batches.is_empty() {
            return Err(Error::Contract(
                "a queue job needs at least one batch".into(),
            ));
        }
        if request.rule_revision.is_empty() {
            return Err(Error::Contract("rule revision cannot be empty".into()));
        }
        let logical_cpus = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);
        let expected_cpu = cpu_plan(request.cpu_preset, logical_cpus);
        if request.profile.simulation.threads != expected_cpu.threads {
            return Err(Error::Contract(format!(
                "profile threads={} does not match the {:?} CPU preset threads={}",
                request.profile.simulation.threads, request.cpu_preset, expected_cpu.threads
            )));
        }
        let identity = validate_supported_binary(request.executable)?;
        let executable_sha256 = sha256_file(request.executable)?;
        let profile_json = serde_json::to_string(request.profile)?;
        let profile_id = self.database.insert_profile(
            request.profile.source_kind,
            &request.batches[0].source_bytes,
            &request.batches[0].generated_bytes,
            &profile_json,
        )?;
        let mut cache_keys = Vec::with_capacity(request.batches.len());
        let batches = request
            .batches
            .into_iter()
            .map(|batch| {
                let generated_sha256 = digest(&batch.generated_bytes);
                let cache_key = exact_cache_key(&ExactCacheInput {
                    generated_bytes: &batch.generated_bytes,
                    executable_sha256: &executable_sha256,
                    simc_version: &identity.simc_version,
                    runtime_revision: request.expected_revision,
                    game_version: &identity.game_version,
                    normalized_schema_version: NORMALIZED_SCHEMA_VERSION,
                    rule_revision: request.rule_revision,
                });
                cache_keys.push(cache_key.clone());
                NewBatch {
                    source_kind: batch.source_kind,
                    source_bytes: batch.source_bytes,
                    generated_bytes: batch.generated_bytes,
                    generated_sha256,
                    cache_key,
                }
            })
            .collect::<Vec<_>>();
        let timeout_millis = u64::try_from(request.timeout.as_millis())
            .map_err(|_| Error::Contract("timeout is too large".into()))?;
        let job_id = self.database.enqueue_job(
            &NewJob {
                profile_id,
                cpu_preset: request.cpu_preset,
                executable_path: request.executable.to_owned(),
                runtime_revision: request.expected_revision.into(),
                executable_sha256,
                simc_version: identity.simc_version,
                game_version: identity.game_version,
                normalized_schema_version: NORMALIZED_SCHEMA_VERSION,
                rule_revision: request.rule_revision.into(),
                timeout_millis,
            },
            &batches,
        )?;
        Ok(EnqueuedJob { job_id, cache_keys })
    }

    pub fn cancel_handle(&self, job_id: i64) -> CancelHandle {
        CancelHandle {
            database_path: self.database_path.clone(),
            job_id,
            token: CancellationToken::default(),
        }
    }

    pub fn recover_and_resume(&mut self) -> Result<RecoveryReport> {
        let running = self.database.running_attempts()?;
        let mut partial_artifacts = Vec::new();
        for attempt in &running {
            if let Some(final_directory) = &attempt.artifact_directory
                && let Some(partial) = quarantine_partial_artifacts(final_directory)?
            {
                self.database
                    .set_attempt_artifact_directory(attempt.id, &partial)?;
                partial_artifacts.push(partial);
            }
        }
        let interrupted_jobs = self.database.recover_interrupted()?;
        for job_id in &interrupted_jobs {
            if self.database.job(*job_id)?.cancel_requested {
                self.database.request_cancel(*job_id)?;
            } else {
                self.database.resume_interrupted(*job_id)?;
            }
        }
        Ok(RecoveryReport {
            interrupted_jobs,
            partial_artifacts,
        })
    }

    pub fn audit_job_artifacts(&self, job_id: i64) -> Result<Vec<ArtifactAuditEntry>> {
        self.database
            .batch_artifacts(job_id)?
            .into_iter()
            .map(|record| {
                let Some(directory) = record.artifact_directory else {
                    return Ok(ArtifactAuditEntry {
                        batch_ordinal: record.ordinal,
                        valid: false,
                        diagnostic: format!(
                            "batch is {} and has no artifact directory",
                            record.state.as_str()
                        ),
                    });
                };
                let Some(expected_manifest) = record.manifest_sha256 else {
                    return Ok(ArtifactAuditEntry {
                        batch_ordinal: record.ordinal,
                        valid: false,
                        diagnostic: "batch has no recorded manifest digest".into(),
                    });
                };
                match verify_artifact_directory(&directory) {
                    Ok(verified) if verified.manifest_sha256 == expected_manifest => {
                        Ok(ArtifactAuditEntry {
                            batch_ordinal: record.ordinal,
                            valid: true,
                            diagnostic: "artifact integrity verified".into(),
                        })
                    }
                    Ok(_) => Ok(ArtifactAuditEntry {
                        batch_ordinal: record.ordinal,
                        valid: false,
                        diagnostic: "manifest digest differs from the repository record".into(),
                    }),
                    Err(error) => Ok(ArtifactAuditEntry {
                        batch_ordinal: record.ordinal,
                        valid: false,
                        diagnostic: error.to_string(),
                    }),
                }
            })
            .collect()
    }

    pub fn run_next(&mut self, token: CancellationToken) -> Result<DispatchResult> {
        self.run_next_with(&SimcBatchExecutor, token)
    }

    pub fn run_next_with<E: BatchExecutor>(
        &mut self,
        executor: &E,
        token: CancellationToken,
    ) -> Result<DispatchResult> {
        let Some(attempt) = self.database.claim_next_attempt()? else {
            return Ok(DispatchResult::Idle);
        };
        let output_directory = self
            .run_root
            .join(format!("job-{}", attempt.job_id))
            .join(format!("batch-{}", attempt.batch_ordinal))
            .join(format!("attempt-{}", attempt.sequence))
            .join("artifacts");
        self.database
            .set_attempt_artifact_directory(attempt.attempt_id, &output_directory)?;

        if let Some(cache) = self.database.cache_entry(&attempt.cache_key)? {
            match verify_artifact_directory(&cache.artifact_directory) {
                Ok(verified)
                    if verified.manifest_sha256 == cache.manifest_sha256
                        && cache_matches(&attempt, &verified.manifest) =>
                {
                    self.database.complete_attempt(
                        attempt.attempt_id,
                        &cache.artifact_directory,
                        &cache.manifest_sha256,
                        true,
                    )?;
                    return Ok(DispatchResult::CacheHit {
                        job_id: attempt.job_id,
                        batch_ordinal: attempt.batch_ordinal,
                    });
                }
                Ok(_) | Err(_) => self.database.remove_cache_entry(&attempt.cache_key)?,
            }
        }

        let log_database = Arc::new(Mutex::new(Database::open(&self.database_path)?));
        let log_failure = Arc::new(Mutex::new(None::<String>));
        let log_database_ref = log_database.clone();
        let log_failure_ref = log_failure.clone();
        let log_cancel = token.clone();
        let attempt_id = attempt.attempt_id;
        let on_log = Arc::new(move |stream: ProcessStream, bytes: &[u8]| {
            let result = log_database_ref
                .lock()
                .map_err(|_| "log database lock was poisoned".to_owned())
                .and_then(|mut database| {
                    database
                        .append_log(
                            attempt_id,
                            match stream {
                                ProcessStream::Stdout => LogStream::Stdout,
                                ProcessStream::Stderr => LogStream::Stderr,
                            },
                            bytes,
                        )
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                });
            if let Err(error) = result {
                if let Ok(mut failure) = log_failure_ref.lock() {
                    *failure = Some(error);
                }
                log_cancel.cancel();
            }
        });
        let execution = executor.execute(
            &attempt,
            &output_directory,
            ExecutionControl {
                cancel: token,
                on_log,
            },
        );
        let log_failure = log_failure
            .lock()
            .map_err(|_| Error::Contract("log failure lock was poisoned".into()))?
            .clone();

        if let Some(failure) = log_failure {
            let artifacts = execution_artifacts(&execution);
            self.database.fail_attempt(
                attempt.attempt_id,
                PersistentState::Failed,
                &format!("bounded log persistence failed: {failure}"),
                artifacts,
            )?;
            return Ok(DispatchResult::Failed {
                job_id: attempt.job_id,
                batch_ordinal: attempt.batch_ordinal,
            });
        }

        match execution {
            Ok(directory) => {
                let verified = match verify_artifact_directory(&directory) {
                    Ok(verified) => verified,
                    Err(error) => {
                        self.database.fail_attempt(
                            attempt.attempt_id,
                            PersistentState::Failed,
                            &format!("partial or corrupt output was rejected: {error}"),
                            Some(&directory),
                        )?;
                        return Ok(DispatchResult::Failed {
                            job_id: attempt.job_id,
                            batch_ordinal: attempt.batch_ordinal,
                        });
                    }
                };
                if !cache_matches(&attempt, &verified.manifest) {
                    self.database.fail_attempt(
                        attempt.attempt_id,
                        PersistentState::Failed,
                        "completed artifact identity does not match the queued batch",
                        Some(&directory),
                    )?;
                    return Ok(DispatchResult::Failed {
                        job_id: attempt.job_id,
                        batch_ordinal: attempt.batch_ordinal,
                    });
                }
                self.database.complete_attempt(
                    attempt.attempt_id,
                    &directory,
                    &verified.manifest_sha256,
                    false,
                )?;
                Ok(DispatchResult::Executed {
                    job_id: attempt.job_id,
                    batch_ordinal: attempt.batch_ordinal,
                })
            }
            Err(failure) => {
                let state = match failure.kind {
                    ExecutionFailureKind::Failed => PersistentState::Failed,
                    ExecutionFailureKind::Canceled => PersistentState::Canceled,
                };
                self.database.fail_attempt(
                    attempt.attempt_id,
                    state,
                    &failure.message,
                    failure.artifacts.as_deref(),
                )?;
                Ok(match failure.kind {
                    ExecutionFailureKind::Failed => DispatchResult::Failed {
                        job_id: attempt.job_id,
                        batch_ordinal: attempt.batch_ordinal,
                    },
                    ExecutionFailureKind::Canceled => DispatchResult::Canceled {
                        job_id: attempt.job_id,
                        batch_ordinal: attempt.batch_ordinal,
                    },
                })
            }
        }
    }
}

pub struct ExactCacheInput<'a> {
    pub generated_bytes: &'a [u8],
    pub executable_sha256: &'a str,
    pub simc_version: &'a str,
    pub runtime_revision: &'a str,
    pub game_version: &'a str,
    pub normalized_schema_version: u32,
    pub rule_revision: &'a str,
}

pub fn exact_cache_key(input: &ExactCacheInput<'_>) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, &EXACT_CACHE_SCHEMA_VERSION.to_le_bytes());
    hash_field(&mut hasher, input.generated_bytes);
    hash_field(&mut hasher, input.executable_sha256.as_bytes());
    hash_field(&mut hasher, input.simc_version.as_bytes());
    hash_field(&mut hasher, input.runtime_revision.as_bytes());
    hash_field(&mut hasher, input.game_version.as_bytes());
    hash_field(&mut hasher, &input.normalized_schema_version.to_le_bytes());
    hash_field(&mut hasher, input.rule_revision.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(bytes);
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn cache_matches(attempt: &ClaimedAttempt, manifest: &simc_adapter::ArtifactManifest) -> bool {
    manifest.source_kind == attempt.source_kind
        && manifest.runtime.channel == "live"
        && manifest.generated_input_sha256 == digest(&attempt.generated_bytes)
        && manifest.executable_sha256 == attempt.executable_sha256
        && manifest.runtime_git_revision == attempt.runtime_revision
        && manifest.runtime.simc_version == attempt.simc_version
        && manifest.runtime.game_version == attempt.game_version
        && manifest.normalized_result_schema_version == Some(attempt.normalized_schema_version)
}

fn execution_artifacts(
    execution: &std::result::Result<PathBuf, ExecutionFailure>,
) -> Option<&Path> {
    match execution {
        Ok(directory) => Some(directory),
        Err(failure) => failure.artifacts.as_deref(),
    }
}

fn quarantine_partial_artifacts(final_directory: &Path) -> Result<Option<PathBuf>> {
    let Some(parent) = final_directory.parent() else {
        return Ok(None);
    };
    if !parent.exists() {
        return Ok(None);
    }
    let mut candidates = fs::read_dir(parent)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".simshredder-run-")
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(std::fs::DirEntry::file_name);
    let Some(candidate) = candidates.into_iter().next() else {
        return Ok(None);
    };
    let metadata = fs::symlink_metadata(candidate.path())?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Contract("partial artifact staging is unsafe".into()));
    }
    let mut destination = parent.join("partial-artifacts");
    let mut suffix = 1_u32;
    while destination.exists() {
        destination = parent.join(format!("partial-artifacts-{suffix}"));
        suffix = suffix
            .checked_add(1)
            .ok_or_else(|| Error::Contract("too many partial artifact directories".into()))?;
    }
    fs::rename(candidate.path(), &destination)?;
    seal_partial_tree(&destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(Some(destination))
}

fn seal_partial_tree(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Contract(
                "partial artifacts contain a symlink".into(),
            ));
        }
        if metadata.is_dir() {
            seal_partial_tree(&entry.path())?;
        } else if metadata.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o400))?;
            }
            #[cfg(windows)]
            {
                let mut permissions = metadata.permissions();
                permissions.set_readonly(true);
                fs::set_permissions(entry.path(), permissions)?;
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o500))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)]
    fn clear_windows_readonly_for_temp_cleanup(path: &Path) {
        // Windows clears only FILE_ATTRIBUTE_READONLY here; the Unix world-writable
        // behavior guarded by this Clippy lint is not compiled on this target.
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn cpu_presets_are_bounded_and_monotonic() {
        let efficient = cpu_plan(CpuPreset::Efficient, 10);
        let balanced = cpu_plan(CpuPreset::Balanced, 10);
        let maximum = cpu_plan(CpuPreset::Maximum, 10);
        assert_eq!(
            efficient,
            CpuPlan {
                threads: 2,
                profileset_work_threads: 1
            }
        );
        assert_eq!(
            balanced,
            CpuPlan {
                threads: 4,
                profileset_work_threads: 2
            }
        );
        assert_eq!(
            maximum,
            CpuPlan {
                threads: 10,
                profileset_work_threads: 5
            }
        );
        assert!(cpu_plan(CpuPreset::Efficient, 0).threads > 0);
    }

    #[test]
    fn exact_cache_changes_for_every_identity_field() {
        let digest = "a".repeat(64);
        let base = ExactCacheInput {
            generated_bytes: b"generated",
            executable_sha256: &digest,
            simc_version: "1210-01",
            runtime_revision: "3487fce",
            game_version: "12.1.0.1",
            normalized_schema_version: 1,
            rule_revision: QUICK_RULE_REVISION,
        };
        let key = exact_cache_key(&base);
        assert_eq!(key, exact_cache_key(&base));
        assert_ne!(
            key,
            exact_cache_key(&ExactCacheInput {
                generated_bytes: b"changed",
                ..base
            })
        );
        assert_ne!(
            key,
            exact_cache_key(&ExactCacheInput {
                normalized_schema_version: 2,
                ..base
            })
        );
        assert_ne!(
            key,
            exact_cache_key(&ExactCacheInput {
                rule_revision: "quick-v2",
                ..base
            })
        );
    }

    #[test]
    fn partial_staging_is_preserved_and_sealed() {
        let temporary = tempfile::tempdir().unwrap();
        let parent = temporary.path().join("attempt-1");
        let staging = parent.join(".simshredder-run-fixture");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("source.simc"), b"source").unwrap();
        let partial = quarantine_partial_artifacts(&parent.join("artifacts"))
            .unwrap()
            .unwrap();
        let preserved = partial.join("source.simc");
        assert!(preserved.is_file());
        assert!(!staging.exists());
        assert!(fs::metadata(&preserved).unwrap().permissions().readonly());
        #[cfg(windows)]
        clear_windows_readonly_for_temp_cleanup(&preserved);
    }
}

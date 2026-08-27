use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use sha2::{Digest, Sha256};
use simc_adapter::{
    ArtifactEntry, ArtifactManifest, ProcessStream, RunStatus, SimcIdentity, sha256_file,
};
use simshredder_domain::SourceKind;
use simshredder_job_runner::{
    BatchExecutor, CancellationToken, DispatchResult, ExecutionControl, ExecutionFailure,
    ExecutionFailureKind, PersistentQueue,
};
use simshredder_storage::{
    ClaimedAttempt, CpuPreset, Database, LogStream, MAX_LOG_BYTES_PER_STREAM, NewBatch, NewJob,
    PersistentState,
};

const CHILD_ENV: &str = "SIMSHREDDER_PHASE2_CRASH_CHILD";
const DATABASE_ENV: &str = "SIMSHREDDER_PHASE2_DATABASE";
const RUN_ROOT_ENV: &str = "SIMSHREDDER_PHASE2_RUN_ROOT";

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fixture_job(profile_id: i64) -> NewJob {
    NewJob {
        profile_id,
        cpu_preset: CpuPreset::Balanced,
        executable_path: PathBuf::from("/fixture/simc"),
        runtime_revision: "3487fce".into(),
        executable_sha256: "a".repeat(64),
        simc_version: "1210-01".into(),
        game_version: "12.1.0.69465".into(),
        normalized_schema_version: 1,
        rule_revision: "quick-v1".into(),
        timeout_millis: 60_000,
    }
}

fn fixture_batches(count: usize, cache_offset: usize) -> Vec<NewBatch> {
    (0..count)
        .map(|ordinal| {
            let source = format!("source-{ordinal}\n").into_bytes();
            let generated = format!("generated-{ordinal}\n").into_bytes();
            NewBatch {
                source_kind: SourceKind::SimcFile,
                source_bytes: source,
                generated_sha256: digest(&generated),
                generated_bytes: generated,
                cache_key: format!("{:064x}", ordinal + 100 + cache_offset),
            }
        })
        .collect()
}

fn enqueue_fixture(database: &mut Database, count: usize) -> i64 {
    enqueue_fixture_with_offset(database, count, 0)
}

fn enqueue_fixture_with_offset(database: &mut Database, count: usize, cache_offset: usize) -> i64 {
    let profile_id = database
        .insert_profile(SourceKind::SimcFile, b"source", b"generated", "{}")
        .unwrap();
    database
        .enqueue_job(
            &fixture_job(profile_id),
            &fixture_batches(count, cache_offset),
        )
        .unwrap()
}

fn write_success_artifacts(attempt: &ClaimedAttempt, directory: &Path) -> String {
    fs::create_dir_all(directory).unwrap();
    let files: [(&str, &[u8], &str); 7] = [
        (
            "source.simc",
            &attempt.source_bytes,
            "text/plain; charset=utf-8",
        ),
        (
            "generated.simc",
            &attempt.generated_bytes,
            "text/plain; charset=utf-8",
        ),
        (
            "stdout.log",
            b"fixture stdout\n",
            "text/plain; charset=utf-8",
        ),
        ("stderr.log", b"", "text/plain; charset=utf-8"),
        ("result.json", b"{}\n", "application/json"),
        (
            "report.html",
            b"<!DOCTYPE html><html></html>\n",
            "text/html; charset=utf-8",
        ),
        ("normalized.json", b"{}\n", "application/json"),
    ];
    let mut artifacts = Vec::new();
    for (name, bytes, media_type) in files {
        fs::write(directory.join(name), bytes).unwrap();
        artifacts.push(ArtifactEntry {
            path: name.into(),
            media_type: media_type.into(),
            bytes: u64::try_from(bytes.len()).unwrap(),
            sha256: digest(bytes),
        });
    }
    let manifest = ArtifactManifest {
        schema_version: 1,
        simshredder_version: "0.1.0".into(),
        normalized_result_schema_version: Some(attempt.normalized_schema_version),
        status: RunStatus::Succeeded,
        source_kind: attempt.source_kind,
        started_unix_millis: 1,
        elapsed_millis: 1,
        runtime: SimcIdentity {
            simc_version: attempt.simc_version.clone(),
            game_version: attempt.game_version.clone(),
            channel: "live".into(),
            hotfix: None,
        },
        runtime_git_revision: attempt.runtime_revision.clone(),
        executable_sha256: attempt.executable_sha256.clone(),
        source_sha256: digest(&attempt.source_bytes),
        generated_input_sha256: digest(&attempt.generated_bytes),
        argv: vec![
            "generated.simc".into(),
            "json2=result.json".into(),
            "html=report.html".into(),
        ],
        exit_code: Some(0),
        stdout_truncated: false,
        stderr_truncated: false,
        failure: None,
        artifacts,
    };
    let mut bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    bytes.push(b'\n');
    fs::write(directory.join("manifest.json"), bytes).unwrap();
    sha256_file(&directory.join("manifest.json")).unwrap()
}

struct FixtureExecutor {
    calls: Arc<AtomicUsize>,
    partial: bool,
    emit_large_log: bool,
}

impl BatchExecutor for FixtureExecutor {
    fn execute(
        &self,
        attempt: &ClaimedAttempt,
        output_directory: &Path,
        control: ExecutionControl,
    ) -> Result<PathBuf, ExecutionFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if control.is_canceled() {
            return Err(ExecutionFailure {
                kind: ExecutionFailureKind::Canceled,
                message: "fixture observed cancellation".into(),
                artifacts: None,
            });
        }
        let log = if self.emit_large_log {
            vec![b'x'; MAX_LOG_BYTES_PER_STREAM + 10]
        } else {
            b"streamed fixture log\n".to_vec()
        };
        control.emit_log(ProcessStream::Stdout, &log);
        if self.partial {
            fs::create_dir_all(output_directory).unwrap();
            fs::write(output_directory.join("source.simc"), &attempt.source_bytes).unwrap();
            return Ok(output_directory.to_owned());
        }
        write_success_artifacts(attempt, output_directory);
        Ok(output_directory.to_owned())
    }
}

struct PanicExecutor;

impl BatchExecutor for PanicExecutor {
    fn execute(
        &self,
        _attempt: &ClaimedAttempt,
        _output_directory: &Path,
        _control: ExecutionControl,
    ) -> Result<PathBuf, ExecutionFailure> {
        panic!("a verified exact cache hit must not execute the batch")
    }
}

struct DiskFullExecutor;

impl BatchExecutor for DiskFullExecutor {
    fn execute(
        &self,
        _attempt: &ClaimedAttempt,
        _output_directory: &Path,
        _control: ExecutionControl,
    ) -> Result<PathBuf, ExecutionFailure> {
        Err(ExecutionFailure {
            kind: ExecutionFailureKind::Failed,
            message: "No space left on device while writing SimulationCraft output".into(),
            artifacts: None,
        })
    }
}

#[test]
#[ignore = "subprocess helper that exits without unwinding"]
fn crash_restart_child() {
    if env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let database_path = PathBuf::from(env::var_os(DATABASE_ENV).unwrap());
    let run_root = PathBuf::from(env::var_os(RUN_ROOT_ENV).unwrap());
    let mut database = Database::open(&database_path).unwrap();
    let attempt = database.claim_next_attempt().unwrap().unwrap();
    assert_eq!(attempt.batch_ordinal, 1);
    let final_directory = run_root
        .join(format!("job-{}", attempt.job_id))
        .join(format!("batch-{}", attempt.batch_ordinal))
        .join(format!("attempt-{}", attempt.sequence))
        .join("artifacts");
    database
        .set_attempt_artifact_directory(attempt.attempt_id, &final_directory)
        .unwrap();
    let staging = final_directory
        .parent()
        .unwrap()
        .join(".simshredder-run-crashed-process");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("source.simc"), &attempt.source_bytes).unwrap();
    fs::write(staging.join("result.json"), b"{partial").unwrap();
    std::process::exit(23);
}

#[test]
fn forced_process_exit_recovers_only_remaining_batches_and_preserves_artifacts() {
    let temporary = tempfile::tempdir().unwrap();
    let database_path = temporary.path().join("state.sqlite3");
    let run_root = temporary.path().join("runs");
    fs::create_dir_all(&run_root).unwrap();
    let mut database = Database::open(&database_path).unwrap();
    let job_id = enqueue_fixture(&mut database, 3);

    let first = database.claim_next_attempt().unwrap().unwrap();
    let first_directory = run_root.join("completed-first");
    let first_manifest = write_success_artifacts(&first, &first_directory);
    database
        .complete_attempt(first.attempt_id, &first_directory, &first_manifest, false)
        .unwrap();
    drop(database);

    let status = Command::new(env::current_exe().unwrap())
        .args(["--ignored", "--exact", "crash_restart_child"])
        .env(CHILD_ENV, "1")
        .env(DATABASE_ENV, &database_path)
        .env(RUN_ROOT_ENV, &run_root)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(23));

    let mut queue = PersistentQueue::open(&database_path, &run_root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&run_root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let recovery = queue.recover_and_resume().unwrap();
    assert_eq!(recovery.interrupted_jobs, vec![job_id]);
    assert_eq!(recovery.partial_artifacts.len(), 1);
    assert!(recovery.partial_artifacts[0].join("source.simc").is_file());
    assert!(first_directory.join("manifest.json").is_file());

    let calls = Arc::new(AtomicUsize::new(0));
    let executor = FixtureExecutor {
        calls: calls.clone(),
        partial: false,
        emit_large_log: false,
    };
    assert_eq!(
        queue
            .run_next_with(&executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed {
            job_id,
            batch_ordinal: 1
        }
    );
    assert_eq!(
        queue
            .run_next_with(&executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed {
            job_id,
            batch_ordinal: 2
        }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        queue.database().job(job_id).unwrap().state,
        PersistentState::Succeeded
    );
    let attempts = queue.database().attempts_for_job(job_id).unwrap();
    assert_eq!(attempts.len(), 4);
    assert_eq!(attempts[0].state, PersistentState::Succeeded);
    assert_eq!(attempts[1].state, PersistentState::Interrupted);
    assert_eq!(attempts[2].state, PersistentState::Succeeded);
    assert_eq!(attempts[3].state, PersistentState::Succeeded);
    assert!(
        queue
            .audit_job_artifacts(job_id)
            .unwrap()
            .iter()
            .all(|entry| entry.valid)
    );

    let cached_job = enqueue_fixture(queue.database_mut(), 3);
    for ordinal in 0..3 {
        assert_eq!(
            queue
                .run_next_with(&PanicExecutor, CancellationToken::default())
                .unwrap(),
            DispatchResult::CacheHit {
                job_id: cached_job,
                batch_ordinal: ordinal
            }
        );
    }
    assert_eq!(
        queue.database().job(cached_job).unwrap().state,
        PersistentState::Succeeded
    );

    fs::remove_file(first_directory.join("normalized.json")).unwrap();
    assert!(!queue.audit_job_artifacts(job_id).unwrap()[0].valid);
    let cache_repair_job = enqueue_fixture(queue.database_mut(), 1);
    let repair_calls = Arc::new(AtomicUsize::new(0));
    let repair_executor = FixtureExecutor {
        calls: repair_calls.clone(),
        partial: false,
        emit_large_log: false,
    };
    assert_eq!(
        queue
            .run_next_with(&repair_executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed {
            job_id: cache_repair_job,
            batch_ordinal: 0
        }
    );
    assert_eq!(repair_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_retry_bounded_log_and_partial_output_are_diagnosable() {
    let temporary = tempfile::tempdir().unwrap();
    let database_path = temporary.path().join("state.sqlite3");
    let run_root = temporary.path().join("runs");
    let mut queue = PersistentQueue::open(&database_path, &run_root).unwrap();
    let job_id = enqueue_fixture(queue.database_mut(), 1);
    let token = CancellationToken::default();
    token.cancel();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = FixtureExecutor {
        calls: calls.clone(),
        partial: false,
        emit_large_log: false,
    };
    assert!(matches!(
        queue.run_next_with(&executor, token).unwrap(),
        DispatchResult::Canceled { .. }
    ));
    assert_eq!(
        queue.database().job(job_id).unwrap().state,
        PersistentState::Canceled
    );

    queue.database_mut().retry_job(job_id).unwrap();
    let large_log_executor = FixtureExecutor {
        calls,
        partial: false,
        emit_large_log: true,
    };
    assert!(matches!(
        queue
            .run_next_with(&large_log_executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed { .. }
    ));
    let successful_attempt = queue.database().attempts_for_job(job_id).unwrap()[1].id;
    assert_eq!(
        queue
            .database()
            .read_log(successful_attempt, LogStream::Stdout)
            .unwrap()
            .len(),
        MAX_LOG_BYTES_PER_STREAM
    );
    assert!(queue.database().attempts_for_job(job_id).unwrap()[1].stdout_log_truncated);

    let partial_job = enqueue_fixture_with_offset(queue.database_mut(), 1, 1_000);
    let partial_executor = FixtureExecutor {
        calls: Arc::new(AtomicUsize::new(0)),
        partial: true,
        emit_large_log: false,
    };
    assert!(matches!(
        queue
            .run_next_with(&partial_executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Failed { .. }
    ));
    let attempt = &queue.database().attempts_for_job(partial_job).unwrap()[0];
    assert_eq!(attempt.state, PersistentState::Failed);
    assert!(
        attempt
            .failure
            .as_deref()
            .unwrap()
            .contains("partial or corrupt")
    );
    assert!(
        attempt
            .artifact_directory
            .as_ref()
            .unwrap()
            .join("source.simc")
            .is_file()
    );

    let disk_full_job = enqueue_fixture_with_offset(queue.database_mut(), 2, 2_000);
    let successful_executor = FixtureExecutor {
        calls: Arc::new(AtomicUsize::new(0)),
        partial: false,
        emit_large_log: false,
    };
    assert!(matches!(
        queue
            .run_next_with(&successful_executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed {
            batch_ordinal: 0,
            ..
        }
    ));
    let completed_directory = queue.database().attempts_for_job(disk_full_job).unwrap()[0]
        .artifact_directory
        .clone()
        .unwrap();
    assert!(matches!(
        queue
            .run_next_with(&DiskFullExecutor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Failed {
            batch_ordinal: 1,
            ..
        }
    ));
    assert!(completed_directory.join("manifest.json").is_file());
    let snapshot = queue.database().job(disk_full_job).unwrap();
    assert_eq!(snapshot.state, PersistentState::Failed);
    assert_eq!(snapshot.succeeded_batches, 1);
    assert!(
        snapshot
            .failure
            .as_deref()
            .unwrap()
            .contains("No space left")
    );
    queue.database_mut().retry_job(disk_full_job).unwrap();
    assert!(matches!(
        queue
            .run_next_with(&successful_executor, CancellationToken::default())
            .unwrap(),
        DispatchResult::Executed {
            batch_ordinal: 1,
            ..
        }
    ));
}

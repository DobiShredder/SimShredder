#![cfg(target_os = "macos")]

use std::{env, fs, path::PathBuf, thread, time::Duration};

use simshredder_job_runner::{
    BatchInput, CancellationToken, DispatchResult, EnqueueRequest, PersistentQueue,
    QUICK_RULE_REVISION, apply_cpu_preset,
};
use simshredder_storage::{CpuPreset, PersistentState};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be in workspace")
        .to_owned()
}

fn required_path(name: &str) -> PathBuf {
    let path = env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must be set for this ignored live test"));
    if path.is_absolute() {
        path
    } else {
        root().join(path)
    }
}

#[test]
#[ignore = "executes the real official SimulationCraft runtime through the persistent queue"]
fn persistent_queue_executes_caches_and_cancels_real_simc() {
    let root = root();
    let executable = required_path("SIMSHREDDER_SIMC_BINARY");
    let revision =
        env::var("SIMSHREDDER_SIMC_REVISION").expect("SIMSHREDDER_SIMC_REVISION must be set");
    let source =
        fs::read_to_string(root.join("test-data/fixtures/profiles/addon-warrior.simc")).unwrap();
    let mut profile = profile_parser::parse_addon_export(&source).unwrap();
    let cpu = apply_cpu_preset(&mut profile, CpuPreset::Balanced, 10);
    assert_eq!(cpu.threads, 4);
    let generated = input_builder::build_input(&profile).unwrap();

    let temporary = tempfile::tempdir().unwrap();
    let database = temporary.path().join("state.sqlite3");
    let runs = temporary.path().join("runs");
    let mut queue = PersistentQueue::open(&database, &runs).unwrap();
    let enqueue =
        |queue: &mut PersistentQueue, profile: &simshredder_domain::Profile, generated: &[u8]| {
            queue
                .enqueue(EnqueueRequest {
                    profile,
                    batches: vec![BatchInput {
                        source_kind: profile.source_kind,
                        source_bytes: source.as_bytes().to_vec(),
                        generated_bytes: generated.to_vec(),
                    }],
                    executable: &executable,
                    expected_revision: &revision,
                    cpu_preset: CpuPreset::Balanced,
                    timeout: Duration::from_secs(60),
                    rule_revision: QUICK_RULE_REVISION,
                })
                .unwrap()
        };

    let first = enqueue(&mut queue, &profile, &generated);
    assert_eq!(
        queue.run_next(CancellationToken::default()).unwrap(),
        DispatchResult::Executed {
            job_id: first.job_id,
            batch_ordinal: 0
        }
    );
    assert_eq!(
        queue.database().job(first.job_id).unwrap().state,
        PersistentState::Succeeded
    );
    assert!(queue.audit_job_artifacts(first.job_id).unwrap()[0].valid);

    let cached = enqueue(&mut queue, &profile, &generated);
    assert_eq!(
        queue.run_next(CancellationToken::default()).unwrap(),
        DispatchResult::CacheHit {
            job_id: cached.job_id,
            batch_ordinal: 0
        }
    );
    assert!(queue.database().attempts_for_job(cached.job_id).unwrap()[0].cache_hit);

    let mut long_profile = profile.clone();
    long_profile.simulation.iterations = 10_000_000;
    let long_generated = input_builder::build_input(&long_profile).unwrap();
    let long_job = enqueue(&mut queue, &long_profile, &long_generated);
    let cancel = queue.cancel_handle(long_job.job_id);
    let token = cancel.token();
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancel.cancel().unwrap();
    });
    assert_eq!(
        queue.run_next(token).unwrap(),
        DispatchResult::Canceled {
            job_id: long_job.job_id,
            batch_ordinal: 0
        }
    );
    cancel_thread.join().unwrap();
    assert_eq!(
        queue.database().job(long_job.job_id).unwrap().state,
        PersistentState::Canceled
    );
    let canceled_attempt = &queue.database().attempts_for_job(long_job.job_id).unwrap()[0];
    assert!(
        canceled_attempt
            .artifact_directory
            .as_ref()
            .unwrap()
            .join("manifest.json")
            .is_file()
    );

    let crashed_process_job = enqueue(&mut queue, &profile, b"not a valid SimC profile\n");
    assert_eq!(
        queue.run_next(CancellationToken::default()).unwrap(),
        DispatchResult::Failed {
            job_id: crashed_process_job.job_id,
            batch_ordinal: 0
        }
    );
    let failed_attempt = &queue
        .database()
        .attempts_for_job(crashed_process_job.job_id)
        .unwrap()[0];
    assert_eq!(failed_attempt.state, PersistentState::Failed);
    assert!(
        failed_attempt
            .artifact_directory
            .as_ref()
            .unwrap()
            .join("manifest.json")
            .is_file()
    );
}

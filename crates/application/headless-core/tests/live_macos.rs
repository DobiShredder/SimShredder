#![cfg(target_os = "macos")]

use std::{env, fs, path::PathBuf, time::Duration};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use simc_adapter::{Error as AdapterError, HeadlessRunRequest, run_headless_quick};
use simshredder_core::{InputFormat, execute, prepare};
use simshredder_domain::SourceKind;

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
#[ignore = "executes the real official SimulationCraft runtime"]
fn addon_and_simc_file_run_to_normalized_immutable_artifacts() {
    let root = root();
    let executable = required_path("SIMSHREDDER_SIMC_BINARY");
    let revision =
        env::var("SIMSHREDDER_SIMC_REVISION").expect("SIMSHREDDER_SIMC_REVISION must be set");
    let temporary = tempfile::tempdir().unwrap();
    for (name, format, expected_player) in [
        ("addon-warrior", InputFormat::AddonExport, "PhaseOneAddon"),
        ("file-warrior", InputFormat::SimcFile, "PhaseOneFile"),
        (
            "advanced-warrior",
            InputFormat::SimcFile,
            "Advanced Warrior",
        ),
    ] {
        let source =
            fs::read_to_string(root.join(format!("test-data/fixtures/profiles/{name}.simc")))
                .unwrap();
        let prepared = prepare(&source, format).unwrap();
        let directory = temporary.path().join(name);
        let result = execute(
            &prepared,
            &executable,
            &revision,
            &directory,
            Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(result.normalized.player.name, expected_player);
        assert_eq!(result.normalized.runtime.channel, "live");
        assert_eq!(result.manifest.artifacts.len(), 7);
        assert!(directory.join("manifest.json").is_file());
        assert!(directory.join("source.simc").is_file());
        assert!(directory.join("generated.simc").is_file());
        assert!(directory.join("result.json").is_file());
        assert!(directory.join("report.html").is_file());
        assert!(directory.join("normalized.json").is_file());
        assert_eq!(
            fs::read(directory.join("source.simc")).unwrap(),
            source.as_bytes()
        );
        assert!(
            fs::read(directory.join("generated.simc"))
                .unwrap()
                .starts_with(source.as_bytes())
        );
        assert!(
            fs::metadata(directory.join("manifest.json"))
                .unwrap()
                .permissions()
                .readonly()
        );
        assert!(
            fs::metadata(directory.join("generated.simc"))
                .unwrap()
                .permissions()
                .readonly()
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(directory.join("manifest.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
        assert!(
            execute(
                &prepared,
                &executable,
                &revision,
                &directory,
                Duration::from_secs(60)
            )
            .is_err(),
            "an existing immutable artifact directory must never be overwritten"
        );
    }

    let failed_directory = temporary.path().join("failed-run");
    let error = run_headless_quick(HeadlessRunRequest {
        executable: &executable,
        expected_revision: &revision,
        source_kind: SourceKind::SimcFile,
        source_bytes: b"not a valid SimC profile\n",
        generated_bytes: b"not a valid SimC profile\n",
        output_directory: &failed_directory,
        timeout: Duration::from_secs(60),
    })
    .unwrap_err();
    assert!(matches!(error, AdapterError::ExecutionFailed { .. }));
    assert!(failed_directory.join("manifest.json").is_file());
    assert!(failed_directory.join("stdout.log").is_file());
    assert!(failed_directory.join("stderr.log").is_file());
    assert!(!failed_directory.join("normalized.json").exists());

    let timeout_directory = temporary.path().join("timed-out-run");
    let source =
        fs::read_to_string(root.join("test-data/fixtures/profiles/file-warrior.simc")).unwrap();
    let prepared = prepare(&source, InputFormat::SimcFile).unwrap();
    let error = execute(
        &prepared,
        &executable,
        &revision,
        &timeout_directory,
        Duration::from_millis(1),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        simshredder_core::Error::Adapter(AdapterError::ExecutionFailed { .. })
    ));
    assert!(timeout_directory.join("manifest.json").is_file());
    assert!(timeout_directory.join("stdout.log").is_file());
    assert!(timeout_directory.join("stderr.log").is_file());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(timeout_directory.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["status"], "failed");
    assert!(manifest["failure"].as_str().unwrap().contains("timed out"));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(timeout_directory.join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
}

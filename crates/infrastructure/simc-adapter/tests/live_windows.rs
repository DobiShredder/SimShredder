#![cfg(target_os = "windows")]

use std::{env, fs, path::PathBuf};

use simc_adapter::{RuntimeManifest, install_windows_archive, run_executable_contract};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be inside the workspace")
        .to_owned()
}

fn required_path(name: &str) -> PathBuf {
    let path = env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must be set for this ignored live test"));
    if path.is_absolute() {
        path
    } else {
        project_root().join(path)
    }
}

fn current_manifest() -> RuntimeManifest {
    serde_json::from_slice(
        &fs::read(required_path("SIMSHREDDER_WINDOWS_MANIFEST"))
            .expect("manifest must be readable"),
    )
    .expect("manifest must be valid")
}

#[test]
#[ignore = "installs and executes the pinned official Windows x64 archive"]
fn official_windows_archive_satisfies_the_full_executable_contract() {
    let archive = required_path("SIMSHREDDER_WINDOWS_ARCHIVE");
    let manifest = current_manifest();
    let install_root = tempfile::tempdir().expect("install root must be available");
    let installed = install_windows_archive(&manifest, &archive, install_root.path())
        .expect("official Windows archive must install without administrator access");
    assert_eq!(installed.identity.channel, "live");

    let root = project_root();
    let quick_golden = root.join(format!(
        "test-data/fixtures/contracts/quick-{}-{}.json",
        manifest.simc_version, manifest.build
    ));
    let profileset_golden = root.join(format!(
        "test-data/fixtures/contracts/profileset-{}-{}.json",
        manifest.simc_version, manifest.build
    ));
    let output = tempfile::tempdir().expect("output directory must be available");
    let report = run_executable_contract(
        &installed.executable,
        &root.join("test-data/fixtures/simc/quick.simc"),
        &root.join("test-data/fixtures/simc/profileset.simc"),
        &quick_golden,
        &profileset_golden,
        output.path(),
    )
    .expect("Windows executable contract must pass");
    assert_eq!(report.quick.exit_code, 0);
    assert_eq!(report.profileset.exit_code, 0);
    assert_eq!(report.invalid_input_exit_code, 60);
    assert!(report.cancel_elapsed_millis < 5_000);
}

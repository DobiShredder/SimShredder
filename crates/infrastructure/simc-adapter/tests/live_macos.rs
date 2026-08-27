#![cfg(target_os = "macos")]

use std::{env, fs, path::PathBuf};

use simc_adapter::{
    RuntimeManifest, download_verified, install_macos_dmg, run_executable_contract,
};

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

fn manifest(path: &PathBuf) -> RuntimeManifest {
    serde_json::from_slice(&fs::read(path).expect("manifest must be readable"))
        .expect("manifest must be valid")
}

#[test]
#[ignore = "downloads a large official nightly artifact"]
fn official_download_is_size_and_hash_verified() {
    let manifest_path = required_path("SIMSHREDDER_SIMC_MANIFEST");
    let temporary = tempfile::tempdir().expect("temporary directory must be available");
    let downloaded = download_verified(&manifest(&manifest_path), temporary.path())
        .expect("official download contract must pass");
    assert!(downloaded.is_file());
}

#[test]
#[ignore = "mounts a real official DMG"]
fn official_dmg_installs_a_valid_live_arm64_runtime() {
    let manifest_path = required_path("SIMSHREDDER_SIMC_MANIFEST");
    let dmg = required_path("SIMSHREDDER_SIMC_DMG");
    let temporary = tempfile::tempdir().expect("temporary directory must be available");
    let installed = install_macos_dmg(&manifest(&manifest_path), &dmg, temporary.path())
        .expect("official DMG install contract must pass");
    assert_eq!(installed.identity.channel, "live");
    assert!(installed.executable.is_file());
}

#[test]
#[ignore = "executes a real official SimulationCraft runtime"]
fn live_executable_contract_matches_golden_fixtures() {
    let executable = required_path("SIMSHREDDER_SIMC_BINARY");
    let revision = env::var("SIMSHREDDER_SIMC_REVISION")
        .expect("SIMSHREDDER_SIMC_REVISION must identify the golden fixture");
    let root = project_root();
    let output = tempfile::tempdir().expect("temporary directory must be available");
    let report = run_executable_contract(
        &executable,
        &root.join("test-data/fixtures/simc/quick.simc"),
        &root.join("test-data/fixtures/simc/profileset.simc"),
        &root.join(format!(
            "test-data/fixtures/contracts/quick-1210-01-{revision}.json"
        )),
        &root.join(format!(
            "test-data/fixtures/contracts/profileset-1210-01-{revision}.json"
        )),
        output.path(),
    )
    .expect("executable contract must pass");
    assert_eq!(report.quick.exit_code, 0);
    assert_eq!(report.profileset.exit_code, 0);
    assert_eq!(report.invalid_input_exit_code, 60);
    assert!(report.cancel_elapsed_millis < 5_000);
}

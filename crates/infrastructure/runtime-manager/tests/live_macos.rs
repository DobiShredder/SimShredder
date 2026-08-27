#![cfg(target_os = "macos")]

use std::{env, fs, path::PathBuf};

use simc_adapter::RuntimeManifest;
use simshredder_runtime_manager::RuntimeManager;

fn required_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must name an existing path"))
}

#[test]
#[ignore = "mounts and installs a pinned official SimulationCraft DMG"]
fn installs_activates_and_diagnoses_the_official_runtime() {
    let manifest_path = required_path("SIMSHREDDER_SIMC_MANIFEST");
    let dmg = required_path("SIMSHREDDER_SIMC_DMG");
    let manifest: RuntimeManifest =
        serde_json::from_slice(&fs::read(manifest_path).expect("manifest must be readable"))
            .expect("manifest must be valid JSON");
    let expected_id = format!("{}-{}", manifest.simc_version, manifest.build);
    let temporary = tempfile::tempdir().expect("temporary root must be available");
    let manager =
        RuntimeManager::open(temporary.path().join("managed")).expect("runtime manager must open");

    let installed = manager
        .install_verified_artifact_and_activate(&manifest, &dmg)
        .expect("official runtime must install and activate");
    assert!(installed.healthy);
    assert_eq!(installed.record.simc_version, manifest.simc_version);
    assert_eq!(installed.record.build, manifest.build);
    assert_eq!(installed.identity.channel, "live");
    assert_eq!(
        manager
            .doctor_active()
            .expect("doctor must succeed")
            .expect("runtime must be active")
            .executable,
        installed.executable
    );
    assert_eq!(
        manager
            .state()
            .expect("state must load")
            .active_id
            .as_deref(),
        Some(expected_id.as_str())
    );

    let recovered = manager
        .install_verified_artifact_and_activate(&manifest, &dmg)
        .expect("a complete existing install must be idempotently recovered");
    assert!(recovered.healthy);
    assert_eq!(recovered.record.id, expected_id);
}

#[test]
#[ignore = "diagnoses a caller-provided managed runtime root"]
fn diagnoses_existing_managed_runtime_root() {
    let root = std::env::var_os("SIMSHREDDER_RUNTIME_ROOT")
        .map(PathBuf::from)
        .expect("SIMSHREDDER_RUNTIME_ROOT must be provided");
    let doctor = RuntimeManager::open(root)
        .expect("manager must open")
        .doctor_active()
        .expect("doctor must run")
        .expect("an active runtime must exist");
    assert!(doctor.healthy);
}

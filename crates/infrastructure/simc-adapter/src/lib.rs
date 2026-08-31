//! SimulationCraft runtime acquisition and executable contract primitives.

mod contract;
mod discovery;
mod error;
#[cfg(target_os = "macos")]
mod macos;
mod manifest;
mod process;
mod result;
mod runner;
mod windows;

pub use contract::{BenchmarkReport, ContractReport, run_benchmark, run_executable_contract};
pub use discovery::{
    AvailableRuntime, NightlyAsset, discover_latest_for_target, discover_latest_macos,
    discover_latest_supported,
};
pub use error::{Error, Result};
#[cfg(target_os = "macos")]
pub use macos::{discover_macos_executables, install_macos_dmg, validate_macos_binary};
pub use manifest::{
    RuntimeManifest, check_artifact_availability, download_available, download_verified,
    sha256_file, verify_artifact,
};
pub use process::{
    CancelOutput, LogChunk, ProcessControl, ProcessOutput, ProcessStream, SimcIdentity,
    cancel_after, parse_identity, run_with_control, run_with_timeout,
};
pub use result::{NORMALIZED_SCHEMA_VERSION, normalize_quick_result};
pub use runner::{
    ArtifactEntry, ArtifactManifest, HeadlessRunRequest, HeadlessRunResult, RunStatus,
    VerifiedArtifacts, run_headless_quick, run_headless_quick_controlled,
    verify_artifact_directory,
};
#[cfg(windows)]
pub use windows::{discover_windows_executables, install_windows_archive, validate_windows_binary};
pub use windows::{extract_windows_archive, validate_windows_pe_x64};

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstalledRuntime {
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub executable_sha256: String,
    pub identity: SimcIdentity,
}

#[cfg(target_os = "macos")]
pub fn validate_supported_binary(executable: &Path) -> Result<SimcIdentity> {
    validate_macos_binary(executable)
}

#[cfg(windows)]
pub fn validate_supported_binary(executable: &Path) -> Result<SimcIdentity> {
    validate_windows_binary(executable)
}

#[cfg(target_os = "macos")]
pub fn install_supported_artifact(
    manifest: &RuntimeManifest,
    artifact: &Path,
    install_root: &Path,
) -> Result<InstalledRuntime> {
    install_macos_dmg(manifest, artifact, install_root)
}

#[cfg(windows)]
pub fn install_supported_artifact(
    manifest: &RuntimeManifest,
    artifact: &Path,
    install_root: &Path,
) -> Result<InstalledRuntime> {
    install_windows_archive(manifest, artifact, install_root)
}

pub const fn supported_executable_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "simc"
    }
    #[cfg(windows)]
    {
        "simc.exe"
    }
}

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("official SimulationCraft artifact is no longer available (HTTP {status}): {url}")]
    ArtifactUnavailable { status: u16, url: String },
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid runtime manifest: {0}")]
    InvalidManifest(String),
    #[error("unsafe or invalid SimulationCraft archive: {0}")]
    UnsafeArchive(String),
    #[error("download exceeded the declared or maximum size")]
    DownloadTooLarge,
    #[error("artifact size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("artifact SHA-256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("no supported nightly asset was found")]
    NightlyAssetMissing,
    #[error("official SimulationCraft nightly listing exceeded 2 MiB")]
    NightlyListingTooLarge,
    #[error("external command {program} failed with status {status}: {stderr}")]
    CommandFailed {
        program: String,
        status: String,
        stderr: String,
    },
    #[error("external command timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("external command timed out after {duration:?} with status {status}")]
    ProcessTimedOut {
        duration: std::time::Duration,
        status: String,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    #[error("external command was canceled after {duration:?} with status {status}")]
    ProcessCanceled {
        duration: std::time::Duration,
        status: String,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    #[error("unsafe DMG content at {0}")]
    UnsafeDmg(PathBuf),
    #[error("SimulationCraft executable contract failed: {0}")]
    Contract(String),
    #[error("the requested runtime is already installed at {0}")]
    AlreadyInstalled(PathBuf),
    #[error("artifact directory already exists: {0}")]
    ArtifactDirectoryExists(PathBuf),
    #[error("SimulationCraft exited with {status}; diagnostics were preserved at {artifacts}")]
    ExecutionFailed { status: String, artifacts: PathBuf },
    #[error("SimulationCraft was canceled; diagnostics were preserved at {artifacts}")]
    ExecutionCanceled { artifacts: PathBuf },
    #[error(
        "SimulationCraft result was rejected ({reason}); diagnostics were preserved at {artifacts}"
    )]
    ResultRejected { reason: String, artifacts: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;

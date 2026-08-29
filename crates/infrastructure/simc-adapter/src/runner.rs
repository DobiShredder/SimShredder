use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::fs::File;

use serde::{Deserialize, Serialize};
use simshredder_domain::{NormalizedQuickResult, SourceKind};

use crate::{
    Error, ProcessControl, Result, SimcIdentity, normalize_quick_result, run_with_control,
    sha256_file, validate_supported_binary,
};

const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_REPORT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub path: String,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub simshredder_version: String,
    pub normalized_result_schema_version: Option<u32>,
    pub status: RunStatus,
    pub source_kind: SourceKind,
    pub started_unix_millis: u128,
    pub elapsed_millis: u128,
    pub runtime: SimcIdentity,
    pub runtime_git_revision: String,
    pub executable_sha256: String,
    pub source_sha256: String,
    pub generated_input_sha256: String,
    /// Exact application rule identity used by queued executions. Direct
    /// adapter callers may omit it, while persistent queue artifacts require it.
    #[serde(default)]
    pub rule_revision: Option<String>,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub failure: Option<String>,
    pub artifacts: Vec<ArtifactEntry>,
}

pub struct HeadlessRunRequest<'a> {
    pub executable: &'a Path,
    pub expected_revision: &'a str,
    pub source_kind: SourceKind,
    pub source_bytes: &'a [u8],
    pub generated_bytes: &'a [u8],
    pub rule_revision: Option<&'a str>,
    pub output_directory: &'a Path,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeadlessRunResult {
    pub directory: PathBuf,
    pub manifest: ArtifactManifest,
    pub normalized: NormalizedQuickResult,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedArtifacts {
    pub manifest: ArtifactManifest,
    pub manifest_sha256: String,
}

pub fn run_headless_quick(request: HeadlessRunRequest<'_>) -> Result<HeadlessRunResult> {
    run_headless_quick_controlled(request, ProcessControl::default())
}

pub fn run_headless_quick_controlled(
    request: HeadlessRunRequest<'_>,
    control: ProcessControl,
) -> Result<HeadlessRunResult> {
    validate_request(&request)?;
    let runtime = validate_supported_binary(request.executable)?;
    let executable_sha256 = sha256_file(request.executable)?;
    let parent = request
        .output_directory
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let parent = fs::canonicalize(parent)?;
    let name = request
        .output_directory
        .file_name()
        .ok_or_else(|| Error::Contract("artifact directory needs a file name".into()))?;
    let final_directory = parent.join(name);
    if final_directory.exists() {
        return Err(Error::ArtifactDirectoryExists(final_directory));
    }

    let staging = tempfile::Builder::new()
        .prefix(".simshredder-run-")
        .tempdir_in(&parent)?;
    write_new(&staging.path().join("source.simc"), request.source_bytes)?;
    write_new(
        &staging.path().join("generated.simc"),
        request.generated_bytes,
    )?;
    let started_unix_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Contract("system clock is before the Unix epoch".into()))?
        .as_millis();
    let argv: Vec<String> = vec![
        "generated.simc".into(),
        "json2=result.json".into(),
        "html=report.html".into(),
    ];
    let output = match run_with_control(
        request.executable,
        &argv,
        staging.path(),
        request.timeout,
        control,
    ) {
        Ok(output) => output,
        Err(Error::ProcessTimedOut {
            duration,
            status,
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        }) => {
            write_new(&staging.path().join("stdout.log"), stdout.as_bytes())?;
            write_new(&staging.path().join("stderr.log"), stderr.as_bytes())?;
            let failure = format!("SimulationCraft timed out with {status}");
            let manifest = build_manifest(
                &request,
                runtime,
                executable_sha256,
                started_unix_millis,
                duration,
                exit_code,
                stdout_truncated,
                stderr_truncated,
                RunStatus::Failed,
                Some(failure.clone()),
                staging.path(),
                None,
            )?;
            write_manifest(staging.path(), &manifest)?;
            seal_files(staging.path())?;
            persist(staging, &final_directory)?;
            return Err(Error::ExecutionFailed {
                status: failure,
                artifacts: final_directory,
            });
        }
        Err(Error::ProcessCanceled {
            duration,
            status: _,
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        }) => {
            write_new(&staging.path().join("stdout.log"), stdout.as_bytes())?;
            write_new(&staging.path().join("stderr.log"), stderr.as_bytes())?;
            let manifest = build_manifest(
                &request,
                runtime,
                executable_sha256,
                started_unix_millis,
                duration,
                exit_code,
                stdout_truncated,
                stderr_truncated,
                RunStatus::Failed,
                Some("SimulationCraft execution was canceled".into()),
                staging.path(),
                None,
            )?;
            write_manifest(staging.path(), &manifest)?;
            seal_files(staging.path())?;
            persist(staging, &final_directory)?;
            return Err(Error::ExecutionCanceled {
                artifacts: final_directory,
            });
        }
        Err(error) => return Err(error),
    };
    write_new(&staging.path().join("stdout.log"), output.stdout.as_bytes())?;
    write_new(&staging.path().join("stderr.log"), output.stderr.as_bytes())?;

    if !output.status.success() {
        let manifest = build_manifest(
            &request,
            runtime,
            executable_sha256,
            started_unix_millis,
            output.elapsed,
            output.status.code(),
            output.stdout_truncated,
            output.stderr_truncated,
            RunStatus::Failed,
            Some(format!("SimulationCraft exited with {}", output.status)),
            staging.path(),
            None,
        )?;
        write_manifest(staging.path(), &manifest)?;
        seal_files(staging.path())?;
        persist(staging, &final_directory)?;
        return Err(Error::ExecutionFailed {
            status: output.status.to_string(),
            artifacts: final_directory,
        });
    }

    let normalized = match read_and_normalize(staging.path(), &runtime, request.expected_revision) {
        Ok(normalized) => normalized,
        Err(error) => {
            let reason = error.to_string();
            let manifest = build_manifest(
                &request,
                runtime,
                executable_sha256,
                started_unix_millis,
                output.elapsed,
                output.status.code(),
                output.stdout_truncated,
                output.stderr_truncated,
                RunStatus::Failed,
                Some(reason.clone()),
                staging.path(),
                None,
            )?;
            write_manifest(staging.path(), &manifest)?;
            seal_files(staging.path())?;
            persist(staging, &final_directory)?;
            return Err(Error::ResultRejected {
                reason,
                artifacts: final_directory,
            });
        }
    };
    let mut normalized_bytes = serde_json::to_vec_pretty(&normalized)?;
    normalized_bytes.push(b'\n');
    write_new(&staging.path().join("normalized.json"), &normalized_bytes)?;

    let manifest = build_manifest(
        &request,
        runtime,
        executable_sha256,
        started_unix_millis,
        output.elapsed,
        output.status.code(),
        output.stdout_truncated,
        output.stderr_truncated,
        RunStatus::Succeeded,
        None,
        staging.path(),
        Some(normalized.schema_version),
    )?;
    write_manifest(staging.path(), &manifest)?;
    seal_files(staging.path())?;
    persist(staging, &final_directory)?;
    Ok(HeadlessRunResult {
        directory: final_directory,
        manifest,
        normalized,
    })
}

pub fn verify_artifact_directory(directory: &Path) -> Result<VerifiedArtifacts> {
    let manifest_path = directory.join("manifest.json");
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| Error::Contract(format!("artifact manifest is missing: {error}")))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > 2 * 1024 * 1024
    {
        return Err(Error::Contract(
            "artifact manifest is not a bounded regular file".into(),
        ));
    }
    let manifest: ArtifactManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    if manifest.schema_version != 1 || manifest.status != RunStatus::Succeeded {
        return Err(Error::Contract(
            "only successful artifact manifest schema 1 can be reused".into(),
        ));
    }
    let expected = [
        "source.simc",
        "generated.simc",
        "stdout.log",
        "stderr.log",
        "result.json",
        "report.html",
        "normalized.json",
    ];
    if manifest.artifacts.len() != expected.len() {
        return Err(Error::Contract(
            "successful artifact manifest has an incomplete file set".into(),
        ));
    }
    for expected_path in expected {
        let entry = manifest
            .artifacts
            .iter()
            .find(|entry| entry.path == expected_path)
            .ok_or_else(|| Error::Contract(format!("artifact {expected_path} is missing")))?;
        let path = directory.join(expected_path);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != entry.bytes
        {
            return Err(Error::Contract(format!(
                "artifact {expected_path} metadata changed"
            )));
        }
        let digest = sha256_file(&path)?;
        if digest != entry.sha256 {
            return Err(Error::HashMismatch {
                expected: entry.sha256.clone(),
                actual: digest,
            });
        }
    }
    if manifest.source_sha256 != sha256_file(&directory.join("source.simc"))?
        || manifest.generated_input_sha256 != sha256_file(&directory.join("generated.simc"))?
    {
        return Err(Error::Contract(
            "artifact manifest input digests do not match files".into(),
        ));
    }
    let mut actual = fs::read_dir(directory)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_file() || file_type.is_symlink() {
                return Err(Error::Contract(
                    "artifact directory contains unsafe entries".into(),
                ));
            }
            entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Contract("artifact name is not UTF-8".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    actual.sort();
    let mut allowed = expected.map(str::to_owned).to_vec();
    allowed.push("manifest.json".into());
    allowed.sort();
    if actual != allowed {
        return Err(Error::Contract(
            "artifact directory contains unexpected files".into(),
        ));
    }
    Ok(VerifiedArtifacts {
        manifest_sha256: sha256_file(&manifest_path)?,
        manifest,
    })
}

fn read_and_normalize(
    directory: &Path,
    runtime: &SimcIdentity,
    expected_revision: &str,
) -> Result<NormalizedQuickResult> {
    validate_report_file(&directory.join("result.json"), "JSON")?;
    validate_report_file(&directory.join("report.html"), "HTML")?;
    let html = fs::read(directory.join("report.html"))?;
    if !html.starts_with(b"<!DOCTYPE html") {
        return Err(Error::Contract(
            "SimulationCraft HTML report has an unexpected document prefix".into(),
        ));
    }
    normalize_quick_result(
        &fs::read(directory.join("result.json"))?,
        runtime,
        expected_revision,
    )
}

fn validate_request(request: &HeadlessRunRequest<'_>) -> Result<()> {
    if request.output_directory.exists() {
        return Err(Error::ArtifactDirectoryExists(
            request.output_directory.to_owned(),
        ));
    }
    if request.source_bytes.is_empty()
        || request.source_bytes.len() > MAX_INPUT_BYTES
        || request.generated_bytes.is_empty()
        || request.generated_bytes.len() > MAX_INPUT_BYTES
        || request.source_bytes.contains(&0)
        || request.generated_bytes.contains(&0)
        || std::str::from_utf8(request.source_bytes).is_err()
        || std::str::from_utf8(request.generated_bytes).is_err()
    {
        return Err(Error::Contract(
            "source and generated input must be bounded UTF-8 without NUL bytes".into(),
        ));
    }
    if !(7..=40).contains(&request.expected_revision.len())
        || !request
            .expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Contract(
            "expected runtime revision is malformed".into(),
        ));
    }
    if request.timeout.is_zero() || request.timeout > Duration::from_secs(24 * 60 * 60) {
        return Err(Error::Contract(
            "run timeout is outside the supported range".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_manifest(
    request: &HeadlessRunRequest<'_>,
    runtime: SimcIdentity,
    executable_sha256: String,
    started_unix_millis: u128,
    elapsed: Duration,
    exit_code: Option<i32>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    status: RunStatus,
    failure: Option<String>,
    directory: &Path,
    normalized_schema: Option<u32>,
) -> Result<ArtifactManifest> {
    let mut artifacts = Vec::new();
    for (path, media_type) in [
        ("source.simc", "text/plain; charset=utf-8"),
        ("generated.simc", "text/plain; charset=utf-8"),
        ("stdout.log", "text/plain; charset=utf-8"),
        ("stderr.log", "text/plain; charset=utf-8"),
        ("result.json", "application/json"),
        ("report.html", "text/html; charset=utf-8"),
        ("normalized.json", "application/json"),
    ] {
        let file = directory.join(path);
        if file.exists() {
            let metadata = fs::symlink_metadata(&file)?;
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Contract(format!("unsafe artifact: {path}")));
            }
            artifacts.push(ArtifactEntry {
                path: path.into(),
                media_type: media_type.into(),
                bytes: metadata.len(),
                sha256: sha256_file(&file)?,
            });
        }
    }
    Ok(ArtifactManifest {
        schema_version: 1,
        simshredder_version: env!("CARGO_PKG_VERSION").into(),
        normalized_result_schema_version: normalized_schema,
        status,
        source_kind: request.source_kind,
        started_unix_millis,
        elapsed_millis: elapsed.as_millis(),
        runtime,
        runtime_git_revision: request.expected_revision.into(),
        executable_sha256,
        source_sha256: digest_bytes(request.source_bytes),
        generated_input_sha256: digest_bytes(request.generated_bytes),
        rule_revision: request.rule_revision.map(str::to_owned),
        argv: vec![
            "generated.simc".into(),
            "json2=result.json".into(),
            "html=report.html".into(),
        ],
        exit_code,
        stdout_truncated,
        stderr_truncated,
        failure,
        artifacts,
    })
}

fn digest_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_report_file(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        Error::Contract(format!("{label} report is missing or unreadable: {error}"))
    })?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_REPORT_BYTES
    {
        return Err(Error::Contract(format!(
            "{label} report is not a bounded regular file"
        )));
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_manifest(directory: &Path, manifest: &ArtifactManifest) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    write_new(&directory.join("manifest.json"), &bytes)
}

fn seal_files(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o400))?;
            }
            #[cfg(not(unix))]
            {
                let mut permissions = metadata.permissions();
                permissions.set_readonly(true);
                fs::set_permissions(entry.path(), permissions)?;
            }
        }
    }
    Ok(())
}

fn persist(staging: tempfile::TempDir, final_directory: &Path) -> Result<()> {
    let staging = staging.keep();
    fs::rename(staging, final_directory)?;
    #[cfg(unix)]
    if let Some(parent) = final_directory.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rejects_unbounded_or_invalid_inputs() {
        let request = HeadlessRunRequest {
            executable: Path::new("simc"),
            expected_revision: "bad",
            source_kind: SourceKind::SimcFile,
            source_bytes: b"source",
            generated_bytes: b"generated",
            rule_revision: None,
            output_directory: Path::new("run"),
            timeout: Duration::from_secs(1),
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn byte_digest_is_stable() {
        assert_eq!(
            digest_bytes(b"fixture"),
            "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d"
        );
    }

    #[test]
    fn sealed_manifest_preserves_the_queue_rule_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let request = HeadlessRunRequest {
            executable: Path::new("simc"),
            expected_revision: "30555ef",
            source_kind: SourceKind::SimcFile,
            source_bytes: b"source",
            generated_bytes: b"generated",
            rule_revision: Some("12.1.0-69465-v1|upgrade_provenance=fixture"),
            output_directory: temporary.path(),
            timeout: Duration::from_secs(1),
        };
        let manifest = build_manifest(
            &request,
            SimcIdentity {
                simc_version: "1210-01".into(),
                game_version: "12.1.0.69465".into(),
                channel: "live".into(),
                hotfix: None,
            },
            "a".repeat(64),
            1,
            Duration::from_millis(1),
            Some(0),
            false,
            false,
            RunStatus::Succeeded,
            None,
            temporary.path(),
            Some(2),
        )
        .unwrap();
        assert_eq!(
            manifest.rule_revision.as_deref(),
            Some("12.1.0-69465-v1|upgrade_provenance=fixture")
        );
    }
}

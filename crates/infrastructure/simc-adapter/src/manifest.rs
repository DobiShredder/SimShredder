use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{
    StatusCode, Url,
    blocking::Client,
    header::{CONTENT_LENGTH, HeaderMap},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AvailableRuntime, Error, Result};

const OFFICIAL_HOST: &str = "downloads.simulationcraft.org";
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub simc_version: String,
    pub game_channel: String,
    pub platform: String,
    pub architecture: String,
    pub build: String,
    pub filename: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

impl RuntimeManifest {
    pub fn validate(&self) -> Result<Url> {
        if self.schema_version != 1 {
            return Err(Error::InvalidManifest("unsupported schema version".into()));
        }
        if self.game_channel != "live" {
            return Err(Error::InvalidManifest(
                "manifest is not for Retail Live".into(),
            ));
        }
        if self.size == 0 || self.size > MAX_ARTIFACT_BYTES {
            return Err(Error::InvalidManifest(
                "artifact size is out of range".into(),
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidManifest("SHA-256 is malformed".into()));
        }
        let expected_name = match (self.platform.as_str(), self.architecture.as_str()) {
            ("macos", "aarch64") => {
                format!("simc-{}-macos-{}.dmg", self.simc_version, self.build)
            }
            ("windows", "x86_64") => format!(
                "simc-{}.{}-win64.7z",
                windows_package_version(&self.simc_version)?,
                self.build
            ),
            _ => {
                return Err(Error::InvalidManifest(
                    "manifest platform and architecture are unsupported".into(),
                ));
            }
        };
        if self.filename != expected_name
            || self.filename.contains('/')
            || self.filename.contains('\\')
        {
            return Err(Error::InvalidManifest(
                "filename does not match identity".into(),
            ));
        }
        let url = Url::parse(&self.url)
            .map_err(|error| Error::InvalidManifest(format!("invalid URL: {error}")))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str() != Some(OFFICIAL_HOST)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.path() != format!("/nightly/{}", self.filename)
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidManifest(
                "URL is outside the official nightly artifact boundary".into(),
            ));
        }
        Ok(url)
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn verify_artifact(manifest: &RuntimeManifest, path: &Path) -> Result<()> {
    manifest.validate()?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidManifest(
            "artifact is not a regular file".into(),
        ));
    }
    if metadata.len() != manifest.size {
        return Err(Error::SizeMismatch {
            expected: manifest.size,
            actual: metadata.len(),
        });
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(Error::HashMismatch {
            expected: manifest.sha256.clone(),
            actual,
        });
    }
    Ok(())
}

pub fn download_verified(manifest: &RuntimeManifest, directory: &Path) -> Result<PathBuf> {
    let url = manifest.validate()?;
    fs::create_dir_all(directory)?;
    let destination = directory.join(&manifest.filename);
    if reusable_cached_artifact(manifest, &destination)? {
        return Ok(destination);
    }

    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(10 * 60))
        .build()?;
    let response = client.get(url.clone()).send()?;
    reject_removed_artifact(response.status(), &url)?;
    let mut response = response.error_for_status()?;
    if let Some(length) = response.content_length()
        && length != manifest.size
    {
        return Err(Error::SizeMismatch {
            expected: manifest.size,
            actual: length,
        });
    }

    let mut temporary = tempfile::Builder::new()
        .prefix(".simc-download-")
        .tempfile_in(directory)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(Error::DownloadTooLarge)?;
        if total > manifest.size || total > MAX_ARTIFACT_BYTES {
            return Err(Error::DownloadTooLarge);
        }
        digest.update(&buffer[..count]);
        temporary.write_all(&buffer[..count])?;
    }
    temporary.flush()?;
    temporary.as_file().sync_all()?;

    if total != manifest.size {
        return Err(Error::SizeMismatch {
            expected: manifest.size,
            actual: total,
        });
    }
    let actual = format!("{:x}", digest.finalize());
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(Error::HashMismatch {
            expected: manifest.sha256.clone(),
            actual,
        });
    }
    temporary
        .persist_noclobber(&destination)
        .map_err(|error| error.error)?;
    verify_artifact(manifest, &destination)?;
    Ok(destination)
}

/// Downloads a runtime selected directly from the official nightly listing.
/// The resulting manifest records the observed size and digest for local cache
/// integrity and installed-runtime recovery; it is not an upstream signature.
pub fn download_available(
    available: &AvailableRuntime,
    directory: &Path,
) -> Result<(RuntimeManifest, PathBuf)> {
    if available.size == 0 || available.size > MAX_ARTIFACT_BYTES {
        return Err(Error::InvalidManifest(
            "discovered artifact size is out of range".into(),
        ));
    }
    let boundary = discovered_manifest(available, "0".repeat(64));
    let url = boundary.validate()?;
    fs::create_dir_all(directory)?;
    let destination = directory.join(&available.asset.filename);
    if destination.exists() {
        let metadata = fs::symlink_metadata(&destination)?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(Error::InvalidManifest(
                "cached nightly artifact is not a regular file".into(),
            ));
        }
        if metadata.len() == available.size {
            let manifest = discovered_manifest(available, sha256_file(&destination)?);
            verify_artifact(&manifest, &destination)?;
            return Ok((manifest, destination));
        }
        fs::remove_file(&destination)?;
    }

    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(10 * 60))
        .build()?;
    let response = client.get(url.clone()).send()?;
    reject_removed_artifact(response.status(), &url)?;
    let mut response = response.error_for_status()?;
    if let Some(length) = declared_content_length(response.headers())
        && length != available.size
    {
        return Err(Error::SizeMismatch {
            expected: available.size,
            actual: length,
        });
    }

    let mut temporary = tempfile::Builder::new()
        .prefix(".simc-download-")
        .tempfile_in(directory)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = response.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(Error::DownloadTooLarge)?;
        if total > available.size || total > MAX_ARTIFACT_BYTES {
            return Err(Error::DownloadTooLarge);
        }
        digest.update(&buffer[..count]);
        temporary.write_all(&buffer[..count])?;
    }
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    if total != available.size {
        return Err(Error::SizeMismatch {
            expected: available.size,
            actual: total,
        });
    }
    let sha256 = format!("{:x}", digest.finalize());
    temporary
        .persist_noclobber(&destination)
        .map_err(|error| error.error)?;
    let manifest = discovered_manifest(available, sha256);
    verify_artifact(&manifest, &destination)?;
    Ok((manifest, destination))
}

fn discovered_manifest(available: &AvailableRuntime, sha256: String) -> RuntimeManifest {
    RuntimeManifest {
        schema_version: 1,
        simc_version: available.asset.simc_version.clone(),
        game_channel: "live".into(),
        platform: available.asset.platform.clone(),
        architecture: available.asset.architecture.clone(),
        build: available.asset.build.clone(),
        filename: available.asset.filename.clone(),
        url: available.url.clone(),
        size: available.size,
        sha256,
    }
}

/// Confirms that an exact previously recorded nightly artifact still exists
/// without downloading its body. Nightly files are routinely replaced upstream.
pub fn check_artifact_availability(manifest: &RuntimeManifest) -> Result<()> {
    let url = manifest.validate()?;
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()?;
    let response = client.head(url.clone()).send()?;
    reject_removed_artifact(response.status(), &url)?;
    let response = response.error_for_status()?;
    if let Some(length) = declared_content_length(response.headers())
        && length != manifest.size
    {
        return Err(Error::SizeMismatch {
            expected: manifest.size,
            actual: length,
        });
    }
    Ok(())
}

fn declared_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
}

fn reject_removed_artifact(status: StatusCode, url: &Url) -> Result<()> {
    if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
        return Err(Error::ArtifactUnavailable {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }
    Ok(())
}

fn reusable_cached_artifact(manifest: &RuntimeManifest, destination: &Path) -> Result<bool> {
    if !destination.exists() {
        return Ok(false);
    }
    match verify_artifact(manifest, destination) {
        Ok(()) => Ok(true),
        Err(error) => {
            let metadata = fs::symlink_metadata(destination)?;
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err(error);
            }
            fs::remove_file(destination)?;
            Ok(false)
        }
    }
}

fn windows_package_version(version: &str) -> Result<String> {
    let mut parts = version.split('-');
    let major = parts.next().unwrap_or_default();
    let minor = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || major.len() != 4
        || minor.len() != 2
        || !major.bytes().all(|byte| byte.is_ascii_digit())
        || !minor.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::InvalidManifest(
            "SimulationCraft version is malformed".into(),
        ));
    }
    Ok(format!("{major}.{minor}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available(size: u64) -> AvailableRuntime {
        AvailableRuntime {
            asset: crate::NightlyAsset {
                filename: "simc-1210-01-macos-3487fce.dmg".into(),
                simc_version: "1210-01".into(),
                build: "3487fce".into(),
                platform: "macos".into(),
                architecture: "aarch64".into(),
            },
            url: "http://downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg"
                .into(),
            size,
        }
    }

    fn manifest(hash: String, size: u64) -> RuntimeManifest {
        RuntimeManifest {
            schema_version: 1,
            simc_version: "1210-01".into(),
            game_channel: "live".into(),
            platform: "macos".into(),
            architecture: "aarch64".into(),
            build: "3487fce".into(),
            filename: "simc-1210-01-macos-3487fce.dmg".into(),
            url: "https://downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg"
                .into(),
            size,
            sha256: hash,
        }
    }

    #[test]
    fn verifies_a_regular_file_by_size_and_hash() {
        let mut artifact = tempfile::NamedTempFile::new().unwrap();
        artifact.write_all(b"fixture").unwrap();
        artifact.flush().unwrap();
        let hash = sha256_file(artifact.path()).unwrap();
        verify_artifact(&manifest(hash, 7), artifact.path()).unwrap();
    }

    #[test]
    fn direct_discovery_reuses_a_bounded_cache_and_records_its_digest() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("simc-1210-01-macos-3487fce.dmg");
        fs::write(&destination, b"fixture").unwrap();
        let (manifest, reused) = download_available(&available(7), temporary.path()).unwrap();
        assert_eq!(reused, destination);
        assert_eq!(manifest.sha256, sha256_file(&destination).unwrap());
        assert_eq!(manifest.size, 7);
    }

    #[test]
    fn direct_download_rejects_a_non_official_candidate_before_network_access() {
        let temporary = tempfile::tempdir().unwrap();
        let mut candidate = available(7);
        candidate.url = "https://example.com/simc.dmg".into();
        assert!(matches!(
            download_available(&candidate, temporary.path()),
            Err(Error::InvalidManifest(_))
        ));
        assert!(fs::read_dir(temporary.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_corrupt_regular_cache_entry_can_be_safely_replaced() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("simc-1210-01-macos-3487fce.dmg");
        fs::write(&destination, b"corrupt").unwrap();
        assert!(!reusable_cached_artifact(&manifest("0".repeat(64), 8), &destination).unwrap());
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_cache_entry_is_rejected_without_removal() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("outside.dmg");
        fs::write(&target, b"fixture").unwrap();
        let destination = temporary.path().join("simc-1210-01-macos-3487fce.dmg");
        symlink(&target, &destination).unwrap();
        let hash = sha256_file(&target).unwrap();

        assert!(reusable_cached_artifact(&manifest(hash, 7), &destination).is_err());
        assert!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&target).unwrap(), b"fixture");
    }

    #[test]
    fn rejects_a_hash_mismatch() {
        let mut artifact = tempfile::NamedTempFile::new().unwrap();
        artifact.write_all(b"fixture").unwrap();
        artifact.flush().unwrap();
        let error = verify_artifact(&manifest("0".repeat(64), 7), artifact.path()).unwrap_err();
        assert!(matches!(error, Error::HashMismatch { .. }));
    }

    #[test]
    fn accepts_exact_official_http_and_rejects_ambiguous_or_mismatched_urls() {
        let mut value = manifest("0".repeat(64), 7);
        value.url =
            "http://downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg".into();
        value.validate().unwrap();

        value.url = "https://example.com/simc-1210-01-macos-3487fce.dmg".into();
        assert!(matches!(value.validate(), Err(Error::InvalidManifest(_))));

        value.url = "https://downloads.simulationcraft.org/nightly/other.dmg".into();
        assert!(matches!(value.validate(), Err(Error::InvalidManifest(_))));

        value.url =
            "https://downloads.simulationcraft.org:8443/nightly/simc-1210-01-macos-3487fce.dmg"
                .into();
        assert!(matches!(value.validate(), Err(Error::InvalidManifest(_))));

        value.url =
            "https://user@downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg"
                .into();
        assert!(matches!(value.validate(), Err(Error::InvalidManifest(_))));
    }

    #[test]
    fn accepts_the_official_windows_x64_archive_identity() {
        let manifest = RuntimeManifest {
            schema_version: 1,
            simc_version: "1210-01".into(),
            game_channel: "live".into(),
            platform: "windows".into(),
            architecture: "x86_64".into(),
            build: "3487fce".into(),
            filename: "simc-1210.01.3487fce-win64.7z".into(),
            url: "https://downloads.simulationcraft.org/nightly/simc-1210.01.3487fce-win64.7z"
                .into(),
            size: 120_746_974,
            sha256: "c212ca4c865819dae06d33c46bd2c01db6fbe5f3abc271577c06046b6ff40c30".into(),
        };
        manifest.validate().unwrap();
    }

    #[test]
    fn head_and_get_share_404_and_410_removed_artifact_classification() {
        let url = Url::parse(
            "http://downloads.simulationcraft.org/nightly/simc-1210-01-macos-3487fce.dmg",
        )
        .unwrap();
        for expected in [StatusCode::NOT_FOUND, StatusCode::GONE] {
            assert!(matches!(
                reject_removed_artifact(expected, &url),
                Err(Error::ArtifactUnavailable { status, .. }) if status == expected.as_u16()
            ));
        }
        reject_removed_artifact(StatusCode::OK, &url).unwrap();
    }

    #[test]
    fn availability_uses_the_declared_head_content_length() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, "227917588".parse().unwrap());
        assert_eq!(declared_content_length(&headers), Some(227_917_588));
    }

    #[test]
    fn removed_candidate_does_not_modify_an_existing_verified_cache_entry() {
        let temporary = tempfile::tempdir().unwrap();
        let destination = temporary.path().join("simc-1210-01-macos-3487fce.dmg");
        fs::write(&destination, b"verified cache").unwrap();
        let cached = manifest(sha256_file(&destination).unwrap(), 14);
        verify_artifact(&cached, &destination).unwrap();

        let url = cached.validate().unwrap();
        assert!(matches!(
            reject_removed_artifact(StatusCode::NOT_FOUND, &url),
            Err(Error::ArtifactUnavailable { status: 404, .. })
        ));
        verify_artifact(&cached, &destination).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"verified cache");
    }
}

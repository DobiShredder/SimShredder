use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Result};

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
    let mut response = client.get(url).send()?.error_for_status()?;
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
}

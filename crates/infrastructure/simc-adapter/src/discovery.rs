use std::{io::Read, time::Duration};

use regex::Regex;
use reqwest::{StatusCode, blocking::Client, header::CONTENT_LENGTH, redirect::Policy};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const NIGHTLY_LISTING_URL: &str = "http://downloads.simulationcraft.org/nightly/?C=M;O=D";
const NIGHTLY_ROOT_URL: &str = "http://downloads.simulationcraft.org/nightly";
const MAX_LISTING_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NightlyAsset {
    pub filename: String,
    pub simc_version: String,
    pub build: String,
    pub platform: String,
    pub architecture: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AvailableRuntime {
    pub asset: NightlyAsset,
    pub url: String,
    pub size: u64,
}

pub fn discover_latest_for_target(
    listing_html: &str,
    platform: &str,
    architecture: &str,
) -> Result<NightlyAsset> {
    let pattern = match (platform, architecture) {
        ("macos", "aarch64") => r#"href="(simc-(\d{4})-(\d{2})-macos-([0-9a-f]{7,40})\.dmg)""#,
        ("windows", "x86_64") => r#"href="(simc-(\d{4})\.(\d{2})\.([0-9a-f]{7,40})-win64\.7z)""#,
        _ => return Err(Error::NightlyAssetMissing),
    };
    let captures = Regex::new(pattern)
        .expect("constant regex must compile")
        .captures(listing_html)
        .ok_or(Error::NightlyAssetMissing)?;

    Ok(NightlyAsset {
        filename: captures[1].to_owned(),
        simc_version: format!("{}-{}", &captures[2], &captures[3]),
        build: captures[4].to_owned(),
        platform: platform.to_owned(),
        architecture: architecture.to_owned(),
    })
}

pub fn discover_latest_macos(listing_html: &str) -> Result<NightlyAsset> {
    discover_latest_for_target(listing_html, "macos", "aarch64")
}

pub fn discover_latest_supported() -> Result<AvailableRuntime> {
    #[cfg(target_os = "macos")]
    let target = ("macos", "aarch64");
    #[cfg(windows)]
    let target = ("windows", "x86_64");
    #[cfg(not(any(target_os = "macos", windows)))]
    compile_error!("SimulationCraft direct discovery supports only macOS and Windows");

    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()?;
    let response = client.get(NIGHTLY_LISTING_URL).send()?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_LISTING_BYTES)
    {
        return Err(Error::NightlyListingTooLarge);
    }
    let mut listing = Vec::new();
    response
        .take(MAX_LISTING_BYTES + 1)
        .read_to_end(&mut listing)?;
    if listing.len() as u64 > MAX_LISTING_BYTES {
        return Err(Error::NightlyListingTooLarge);
    }
    let listing = std::str::from_utf8(&listing)
        .map_err(|_| Error::InvalidManifest("nightly listing is not UTF-8".into()))?;
    let asset = discover_latest_for_target(listing, target.0, target.1)?;
    let url = format!("{NIGHTLY_ROOT_URL}/{}", asset.filename);
    let response = client.head(&url).send()?;
    if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
        return Err(Error::ArtifactUnavailable {
            status: response.status().as_u16(),
            url,
        });
    }
    let response = response.error_for_status()?;
    let size = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|size| *size > 0 && *size <= MAX_ARTIFACT_BYTES)
        .ok_or_else(|| {
            Error::InvalidManifest("nightly artifact has no bounded Content-Length".into())
        })?;
    Ok(AvailableRuntime { asset, url, size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_first_date_sorted_asset_for_each_supported_target() {
        let listing = r#"
          <a href="simc-1210.01.3487fce-win64.7z">latest windows</a>
          <a href="simc-1210-01-macos-3487fce.dmg">latest mac</a>
          <a href="simc-1205-01-macos-ac79c0f.dmg">old mac</a>
        "#;
        let mac = discover_latest_macos(listing).unwrap();
        assert_eq!(mac.filename, "simc-1210-01-macos-3487fce.dmg");
        assert_eq!(mac.build, "3487fce");
        let windows = discover_latest_for_target(listing, "windows", "x86_64").unwrap();
        assert_eq!(windows.filename, "simc-1210.01.3487fce-win64.7z");
        assert_eq!(windows.simc_version, "1210-01");
    }

    #[test]
    fn ignores_malformed_and_wrong_target_assets() {
        let listing = r#"<a href="simc-1210.01.3487fce-winarm64.7z">arm</a>"#;
        assert!(matches!(
            discover_latest_for_target(listing, "windows", "x86_64"),
            Err(Error::NightlyAssetMissing)
        ));
        assert!(matches!(
            discover_latest_for_target(listing, "linux", "x86_64"),
            Err(Error::NightlyAssetMissing)
        ));
    }
}

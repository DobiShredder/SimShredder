use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NightlyAsset {
    pub filename: String,
    pub simc_version: String,
    pub build: String,
}

/// Selects the first supported macOS entry from the upstream date-descending listing.
pub fn discover_latest_macos(listing_html: &str) -> Result<NightlyAsset> {
    let pattern = Regex::new(r#"href="(simc-(\d{4})-(\d{2})-macos-([0-9a-f]{7,40})\.dmg)""#)
        .expect("constant regex must compile");
    let captures = pattern
        .captures(listing_html)
        .ok_or(Error::NightlyAssetMissing)?;

    Ok(NightlyAsset {
        filename: captures[1].to_owned(),
        simc_version: format!("{}-{}", &captures[2], &captures[3]),
        build: captures[4].to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_first_date_sorted_macos_asset() {
        let listing = r#"
          <a href="simc-1210.01.aaaaaaa-win64.7z">windows</a>
          <a href="simc-1210-01-macos-3487fce.dmg">latest mac</a>
          <a href="simc-1205-01-macos-ac79c0f.dmg">old mac</a>
        "#;
        assert_eq!(
            discover_latest_macos(listing).unwrap(),
            NightlyAsset {
                filename: "simc-1210-01-macos-3487fce.dmg".into(),
                simc_version: "1210-01".into(),
                build: "3487fce".into(),
            }
        );
    }

    #[test]
    fn ignores_malformed_and_non_macos_assets() {
        let listing = r#"<a href="simc-1210.01.3487fce-win64.7z">windows</a>"#;
        assert!(matches!(
            discover_latest_macos(listing),
            Err(Error::NightlyAssetMissing)
        ));
    }
}

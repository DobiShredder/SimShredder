use std::{fs, path::PathBuf};

use simc_adapter::{SimcIdentity, normalize_quick_result};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be in workspace")
        .to_owned()
}

#[test]
fn json2_fixture_normalizes_to_versioned_golden() {
    let root = root();
    let result = normalize_quick_result(
        &fs::read(root.join("test-data/fixtures/reports/quick-1210-01-3487fce.min.json")).unwrap(),
        &SimcIdentity {
            simc_version: "1210-01".into(),
            game_version: "12.1.0.69465".into(),
            channel: "live".into(),
            hotfix: Some("2026-08-24/69465".into()),
        },
        "3487fce",
    )
    .unwrap();
    let actual = serde_json::to_value(result).unwrap();
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("test-data/fixtures/contracts/normalized-quick-1210-01-3487fce.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
#[ignore = "requires SIMSHREDDER_ACTUAL_RESULT_JSON from an official local SimC run"]
fn actual_official_report_normalizes_detailed_sections() {
    let path = std::env::var_os("SIMSHREDDER_ACTUAL_RESULT_JSON")
        .map(PathBuf::from)
        .expect("SIMSHREDDER_ACTUAL_RESULT_JSON must name an official report");
    let bytes = fs::read(path).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let simc_version = document["version"].as_str().unwrap();
    let revision = document["git_revision"].as_str().unwrap();
    let game_version = document["sim"]["options"]["dbc"]["Live"]["wow_version"]
        .as_str()
        .unwrap();
    let result = normalize_quick_result(
        &bytes,
        &SimcIdentity {
            simc_version: simc_version.into(),
            game_version: game_version.into(),
            channel: "live".into(),
            hotfix: None,
        },
        revision,
    )
    .unwrap();
    assert!(!result.actions.is_empty());
    assert!(!result.buffs.is_empty());
    assert!(!result.apl_sequence.is_empty());
    assert_eq!(result.schema_version, 2);
}

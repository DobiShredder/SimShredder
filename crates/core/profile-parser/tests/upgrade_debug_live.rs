use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

use profile_parser::{parse_addon_export, parse_document};
use serde::Deserialize;
use simshredder_domain::{GearSlot, ItemUpgradePath, UpgradeCostKind, UpgradeTargetQuote};

const FIXTURE_ENV: &str = "SIMSHREDDER_RETAIL_UPGRADE_DEBUG_FIXTURE";
const ORACLE_ENV: &str = "SIMSHREDDER_RETAIL_UPGRADE_DEBUG_ORACLE";
const MAX_ORACLE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReportedCostSemantics {
    CumulativeFromCurrent,
    EdgeFromPrevious,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Oracle {
    schema: u32,
    captured_at: String,
    addon_contract_commit: String,
    reported_cost_semantics: ReportedCostSemantics,
    evidence: Vec<String>,
    items: Vec<OracleItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OracleItem {
    slot: GearSlot,
    bag_index: Option<usize>,
    item_id: u32,
    current_upgrade_level: u8,
    current_item_level: u32,
    targets: Vec<OracleTarget>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OracleTarget {
    target_level: u8,
    target_item_level: u32,
    effective_costs: BTreeMap<String, u32>,
    discount_flags: BTreeMap<String, bool>,
}

#[test]
#[ignore = "requires a private Retail Live /simc debug export and pinned contract-evidence oracle"]
fn retail_upgrade_debug_matches_pinned_contract_evidence() {
    let fixture_path = std::env::var(FIXTURE_ENV)
        .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the untouched /simc debug export"));
    let oracle_path = std::env::var(ORACLE_ENV)
        .unwrap_or_else(|_| panic!("set {ORACLE_ENV} to the manual vendor observation JSON"));
    let fixture = read_bounded_regular(Path::new(&fixture_path), 2 * 1024 * 1024);
    let oracle_bytes = read_bounded_regular(Path::new(&oracle_path), MAX_ORACLE_BYTES);
    let source = std::str::from_utf8(&fixture).expect("fixture must be UTF-8");
    let oracle: Oracle = serde_json::from_slice(&oracle_bytes).expect("oracle must match schema");
    assert_eq!(oracle.schema, 1);
    assert!(!oracle.captured_at.trim().is_empty());
    assert!(!oracle.evidence.is_empty());
    let oracle_parent = Path::new(&oracle_path).parent().unwrap_or(Path::new("."));
    for evidence in &oracle.evidence {
        let relative = Path::new(evidence);
        assert!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
        );
        let _ = read_bounded_regular(&oracle_parent.join(relative), 16 * 1024 * 1024);
    }

    let document = parse_document(source).expect("debug fixture must be a valid lossless document");
    assert_eq!(document.as_bytes(), fixture);
    let profile = parse_addon_export(source).expect("debug fixture must be a Retail AddOn export");
    let metadata = profile
        .upgrade_metadata
        .expect("debug fixture must contain upgrade metadata");
    assert!(metadata.item_upgrade_diagnostics.is_empty());
    let provenance = metadata.provenance.expect("debug fixture needs provenance");
    assert_eq!(provenance.contract_commit, oracle.addon_contract_commit);
    assert_eq!(
        provenance.wow_build,
        profile.addon.expect("AddOn headers").wow_build
    );
    assert_eq!(metadata.item_upgrade_paths.len(), oracle.items.len());
    assert!(oracle.items.iter().any(|item| item.bag_index.is_none()));
    assert!(oracle.items.iter().any(|item| item.bag_index.is_some()));

    for expected in &oracle.items {
        let path = metadata
            .item_upgrade_paths
            .iter()
            .find(|path| {
                path.slot == expected.slot
                    && path.bag_index == expected.bag_index
                    && path.item_id == expected.item_id
            })
            .expect("oracle item must have one exact parsed path");
        assert_eq!(path.current_item_level, Some(expected.current_item_level));
        assert_eq!(
            infer_current_level(path),
            Some(expected.current_upgrade_level)
        );
        for target in &expected.targets {
            let quote = path
                .targets
                .iter()
                .find(|quote| quote.target_level == target.target_level)
                .expect("observed target must exist in the AddOn quote");
            assert_eq!(
                target_item_level(path, quote),
                Some(target.target_item_level)
            );
            assert_eq!(discount_flags(quote), target.discount_flags);
            assert_eq!(
                effective_costs(
                    path,
                    expected.current_upgrade_level,
                    target.target_level,
                    &oracle.reported_cost_semantics,
                ),
                target.effective_costs
            );
        }
    }
}

#[test]
#[ignore = "prints a private observation worksheet from a caller-provided Retail Live /simc debug export"]
fn print_retail_upgrade_debug_observation_worksheet() {
    let fixture_path = std::env::var(FIXTURE_ENV)
        .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the untouched /simc debug export"));
    let fixture = read_bounded_regular(Path::new(&fixture_path), 2 * 1024 * 1024);
    let source = std::str::from_utf8(&fixture).expect("fixture must be UTF-8");
    let profile = parse_addon_export(source).expect("fixture must be a Retail AddOn export");
    let metadata = profile
        .upgrade_metadata
        .expect("fixture must contain upgrade metadata");
    let provenance = metadata.provenance.expect("fixture needs provenance");
    let items = metadata
        .item_upgrade_paths
        .iter()
        .map(|path| {
            let targets = path
                .targets
                .iter()
                .map(|target| {
                    serde_json::json!({
                        "targetLevel": target.target_level,
                        "itemLevelIncrement": target.item_level_increment,
                        "calculatedTargetItemLevelForReview": target_item_level(path, target),
                        "reportedCosts": reported_costs(target),
                        "reportedDiscountFlags": discount_flags(target),
                        "observedTargetItemLevel": null,
                        "observedEffectiveCosts": null,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "slot": path.slot,
                "bagIndex": path.bag_index,
                "itemId": path.item_id,
                "sourceLine": path.source_line,
                "currentItemLevel": path.current_item_level,
                "observedCurrentUpgradeLevel": null,
                "targets": targets,
            })
        })
        .collect::<Vec<_>>();
    let worksheet = serde_json::json!({
        "worksheetSchema": 1,
        "instructions": "Record the current rank and the vendor UI values for at least two target ranks; do not treat calculated values as observations.",
        "addonContractCommit": provenance.contract_commit,
        "captureMode": provenance.capture_mode,
        "addonVersion": provenance.addon_version,
        "wowVersion": provenance.wow_version,
        "wowBuild": provenance.wow_build,
        "toc": provenance.toc,
        "observedReportedCostSemantics": null,
        "evidenceFiles": [],
        "diagnostics": metadata.item_upgrade_diagnostics,
        "items": items,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&worksheet).expect("worksheet serialization")
    );
}

#[test]
fn oracle_math_handles_signed_levels_and_both_reported_cost_semantics() {
    let source = "# SimC Addon 12.1.0-03\n# WoW 12.1.0.69497, TOC 120100\nrogue=Tester\nlevel=90\nrace=human\nrole=attack\nspec=subtlety\n# Helm (289)\n# upgrade_levels=1:-3:c#3444#5#0:/3:3:c#3444#10#1:/4:6:c#3444#15#1:i#274476#1#0\nhead=,id=250006,ilevel=289\n";
    let profile = parse_addon_export(source).unwrap();
    let metadata = profile.upgrade_metadata.unwrap();
    let path = &metadata.item_upgrade_paths[0];
    assert_eq!(infer_current_level(path), Some(2));
    assert_eq!(target_item_level(path, &path.targets[0]), Some(286));
    assert_eq!(target_item_level(path, &path.targets[2]), Some(295));
    assert_eq!(
        effective_costs(path, 2, 4, &ReportedCostSemantics::CumulativeFromCurrent),
        BTreeMap::from([("c:3444".into(), 15), ("i:274476".into(), 1)])
    );
    assert_eq!(
        effective_costs(path, 2, 4, &ReportedCostSemantics::EdgeFromPrevious),
        BTreeMap::from([("c:3444".into(), 25), ("i:274476".into(), 1)])
    );
    assert_eq!(
        discount_flags(&path.targets[2]),
        BTreeMap::from([("c:3444".into(), true), ("i:274476".into(), false)])
    );
}

fn discount_flags(target: &UpgradeTargetQuote) -> BTreeMap<String, bool> {
    target
        .currency_costs
        .iter()
        .chain(target.item_costs.iter())
        .map(|cost| {
            let prefix = match cost.kind {
                UpgradeCostKind::Currency => "c",
                UpgradeCostKind::Item => "i",
            };
            (format!("{prefix}:{}", cost.id), cost.discounted)
        })
        .collect()
}

fn reported_costs(target: &UpgradeTargetQuote) -> BTreeMap<String, u32> {
    let mut costs = BTreeMap::<String, u32>::new();
    for cost in target.currency_costs.iter().chain(target.item_costs.iter()) {
        let prefix = match cost.kind {
            UpgradeCostKind::Currency => "c",
            UpgradeCostKind::Item => "i",
        };
        let key = format!("{prefix}:{}", cost.id);
        let next = costs
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(cost.amount)
            .expect("reported worksheet cost must not overflow");
        costs.insert(key, next);
    }
    costs
}

fn read_bounded_regular(path: &Path, maximum: u64) -> Vec<u8> {
    let metadata = fs::symlink_metadata(path).expect("evidence path must exist");
    assert!(metadata.file_type().is_file());
    assert!(!metadata.file_type().is_symlink());
    assert!(metadata.len() <= maximum);
    fs::read(path).expect("evidence must be readable")
}

fn infer_current_level(path: &ItemUpgradePath) -> Option<u8> {
    if let Some(first) = path.targets.first()
        && first.target_level > 0
        && path.targets.iter().enumerate().all(|(index, target)| {
            usize::from(target.target_level) == usize::from(first.target_level) + index
                && target.item_level_increment > 0
        })
    {
        return first.target_level.checked_sub(1);
    }
    let maximum = path
        .targets
        .iter()
        .map(|target| target.target_level)
        .max()?;
    let candidates = (1..=maximum.saturating_add(1))
        .filter(|candidate| {
            !path
                .targets
                .iter()
                .any(|target| target.target_level == *candidate)
                && path.targets.iter().all(|target| {
                    (target.target_level < *candidate && target.item_level_increment <= 0)
                        || (target.target_level > *candidate && target.item_level_increment >= 0)
                })
        })
        .collect::<Vec<_>>();
    (candidates.len() == 1).then_some(candidates[0])
}

fn target_item_level(path: &ItemUpgradePath, target: &UpgradeTargetQuote) -> Option<u32> {
    let current = i64::from(path.current_item_level?);
    u32::try_from(current.checked_add(i64::from(target.item_level_increment))?)
        .ok()
        .filter(|level| *level > 0)
}

fn effective_costs(
    path: &ItemUpgradePath,
    current_level: u8,
    target_level: u8,
    semantics: &ReportedCostSemantics,
) -> BTreeMap<String, u32> {
    if target_level <= current_level {
        return BTreeMap::new();
    }
    let quotes = match semantics {
        ReportedCostSemantics::CumulativeFromCurrent => path
            .targets
            .iter()
            .filter(|quote| quote.target_level == target_level)
            .collect::<Vec<_>>(),
        ReportedCostSemantics::EdgeFromPrevious => path
            .targets
            .iter()
            .filter(|quote| {
                quote.target_level > current_level && quote.target_level <= target_level
            })
            .collect::<Vec<_>>(),
    };
    let mut costs = BTreeMap::<String, u32>::new();
    for cost in quotes
        .into_iter()
        .flat_map(|quote| quote.currency_costs.iter().chain(quote.item_costs.iter()))
    {
        let prefix = match cost.kind {
            UpgradeCostKind::Currency => "c",
            UpgradeCostKind::Item => "i",
        };
        let key = format!("{prefix}:{}", cost.id);
        let next = costs
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(cost.amount)
            .expect("oracle cost must not overflow");
        costs.insert(key, next);
    }
    costs
}

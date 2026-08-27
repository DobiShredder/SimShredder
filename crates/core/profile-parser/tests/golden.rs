use std::{fs, path::PathBuf};

use profile_parser::{parse_addon_export, parse_simc_file};
use serde_json::json;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("crate must be in workspace")
        .to_owned()
}

#[test]
fn addon_and_simc_parser_projection_matches_golden() {
    let root = root();
    let addon = parse_addon_export(
        &fs::read_to_string(root.join("test-data/fixtures/profiles/addon-warrior.simc")).unwrap(),
    )
    .unwrap();
    let simc = parse_simc_file(
        &fs::read_to_string(root.join("test-data/fixtures/profiles/file-warrior.simc")).unwrap(),
    )
    .unwrap();
    let projection = json!({
        "addon": {
            "source_kind": addon.source_kind,
            "channel": addon.channel,
            "metadata": addon.addon,
            "class": addon.class,
            "name": addon.name,
            "level": addon.level,
            "race": addon.race,
            "region": addon.region,
            "server": addon.server,
            "role": addon.role,
            "specialization": addon.specialization,
            "scalar_options": addon.scalar_options,
            "talents": addon.talents,
            "equipped_ids": addon.equipped.iter().map(|(slot, item)| (slot.simc_token(), item.id)).collect::<std::collections::BTreeMap<_, _>>(),
            "bag_items": addon.bag_items,
            "actions": addon.actions,
            "simulation": addon.simulation,
        },
        "simc_file": {
            "source_kind": simc.source_kind,
            "channel": simc.channel,
            "metadata": simc.addon,
            "class": simc.class,
            "name": simc.name,
            "level": simc.level,
            "race": simc.race,
            "region": simc.region,
            "server": simc.server,
            "role": simc.role,
            "specialization": simc.specialization,
            "scalar_options": simc.scalar_options,
            "talents": simc.talents,
            "equipped_ids": simc.equipped.iter().map(|(slot, item)| (slot.simc_token(), item.id)).collect::<std::collections::BTreeMap<_, _>>(),
            "bag_items": simc.bag_items,
            "actions": simc.actions,
            "simulation": simc.simulation,
        }
    });
    let golden: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("test-data/fixtures/contracts/profile-parser.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(projection, golden);
}

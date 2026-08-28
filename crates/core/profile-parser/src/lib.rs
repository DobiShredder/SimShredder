//! Lossless SimulationCraft document parsing and a typed single-player projection.

mod document;
mod policy;

pub use document::{
    DirectiveOperator, SimcDirective, SimcDocument, SimcLine, SimcLineKind, parse_document,
};
pub use policy::{
    CompatibilityCategory, CompatibilityDiagnostic, CompatibilityReport, analyze_compatibility,
};

use std::{collections::BTreeMap, str::FromStr};

use regex::Regex;
use simshredder_domain::{
    ActionDirective, AddonMetadata, BagItem, CharacterClass, GameChannel, GearSlot, Item, Profile,
    Role, SimulationOptions, SlotHighWatermark, SourceKind, TalentLoadout, UpgradeMetadata,
};
use thiserror::Error;

pub(crate) const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("input exceeds the {MAX_INPUT_BYTES}-byte limit")]
    InputTooLarge,
    #[error("line {line}: {message}")]
    Invalid { line: usize, message: String },
    #[error("missing required field: {0}")]
    Missing(&'static str),
}

pub type Result<T> = std::result::Result<T, ParseError>;

#[derive(Default)]
struct ProfileDraft {
    addon: Option<AddonMetadata>,
    class: Option<CharacterClass>,
    name: Option<String>,
    level: Option<u16>,
    race: Option<String>,
    region: Option<String>,
    server: Option<String>,
    role: Option<Option<Role>>,
    specialization: Option<String>,
    scalar_options: BTreeMap<String, String>,
    talents: BTreeMap<String, String>,
    saved_talent_loadouts: Vec<TalentLoadout>,
    equipped: BTreeMap<GearSlot, Item>,
    bag_items: Vec<BagItem>,
    upgrade_metadata: UpgradeMetadata,
    actions: Vec<ActionDirective>,
    simulation: SimulationOptions,
}

pub fn parse_addon_export(input: &str) -> Result<Profile> {
    let document = parse_document(input)?;
    project_profile(&document, SourceKind::AddonExport)
}

pub fn parse_simc_file(input: &str) -> Result<Profile> {
    let document = parse_document(input)?;
    project_profile(&document, SourceKind::SimcFile)
}

pub fn project_profile(document: &SimcDocument, source_kind: SourceKind) -> Result<Profile> {
    let compatibility = analyze_compatibility(document);
    if let Some(blocker) = compatibility.first_blocker() {
        let key = blocker
            .key
            .as_deref()
            .map(|key| format!("{key}: "))
            .unwrap_or_default();
        return invalid(blocker.line, format!("{key}{}", blocker.reason));
    }

    let addon_pattern = Regex::new(r"^# SimC Addon (\S+)$").expect("constant regex");
    let wow_pattern = Regex::new(r"^# WoW ([0-9]+(?:\.[0-9]+){2})\.([0-9]+), TOC ([0-9]+)$")
        .expect("constant regex");
    let mut draft = ProfileDraft {
        simulation: SimulationOptions::default(),
        ..ProfileDraft::default()
    };
    let mut addon_version = None;
    let mut wow_metadata = None;
    let mut in_bag_section = false;
    let mut pending_bag_name = None;
    let mut pending_equipped_name = None;
    let mut pending_saved_loadout_name = None;

    for document_line in &document.lines {
        let line_number = document_line.number;
        let line = document_line.raw.trim();
        if line.is_empty() {
            continue;
        }
        if contains_channel_marker(line) {
            return invalid(line_number, "Classic, PTR and Beta inputs are unsupported");
        }
        if let Some(captures) = addon_pattern.captures(line) {
            addon_version = Some(captures[1].to_owned());
            continue;
        }
        if let Some(captures) = wow_pattern.captures(line) {
            let wow_build = parse_number::<u32>(&captures[2], line_number, "WoW build")?;
            let toc = parse_number::<u32>(&captures[3], line_number, "TOC")?;
            wow_metadata = Some((captures[1].to_owned(), wow_build, toc));
            continue;
        }
        if line == "### Gear from Bags" {
            in_bag_section = true;
            pending_bag_name = None;
            continue;
        }
        if line.starts_with("### ") {
            in_bag_section = false;
            pending_bag_name = None;
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            let comment = comment.trim();
            if let Some((key, value)) = comment.split_once('=')
                && matches!(
                    key.trim(),
                    "upgrade_currencies" | "upgrade_achievements" | "slot_high_watermarks"
                )
            {
                project_upgrade_metadata(
                    &mut draft.upgrade_metadata,
                    key.trim(),
                    value.trim(),
                    line_number,
                )?;
                continue;
            }
            if in_bag_section {
                if comment.is_empty() {
                    pending_bag_name = None;
                } else if let Some((key, value)) = comment.split_once('=') {
                    if let Some(slot) = parse_gear_slot(key.trim()) {
                        let mut item = parse_item(slot, value.trim(), line_number)?;
                        item.name.clone_from(&pending_bag_name);
                        draft.bag_items.push(BagItem {
                            item,
                            name: pending_bag_name.take(),
                        });
                    }
                } else if !comment.starts_with("upgrade_levels=") {
                    pending_bag_name = Some(if looks_like_item_comment(comment) {
                        item_name_from_comment(comment)
                    } else {
                        comment.to_owned()
                    });
                }
            } else {
                if let Some(name) = comment.strip_prefix("Saved Loadout:") {
                    pending_saved_loadout_name = Some(name.trim().to_owned());
                } else if let Some(value) = comment.strip_prefix("talents=")
                    && let Some(name) = pending_saved_loadout_name.take()
                {
                    draft.saved_talent_loadouts.push(TalentLoadout {
                        name,
                        value: value.trim().to_owned(),
                    });
                } else if looks_like_item_comment(comment) {
                    pending_equipped_name = Some(item_name_from_comment(comment));
                }
            }
            continue;
        }

        let (key, value) = line.split_once('=').ok_or_else(|| ParseError::Invalid {
            line: line_number,
            message: "expected a key=value directive".into(),
        })?;
        let key = key.trim();
        let value = value.trim();
        if let Some(class) = parse_class(key) {
            if draft.class.is_some() {
                return invalid(line_number, "multiple player actors are unsupported");
            }
            draft.class = Some(class);
            draft.name = Some(parse_player_name(value, line_number)?);
        } else if let Some(slot) = parse_gear_slot(key) {
            let mut item = parse_item(slot, value, line_number)?;
            item.name = pending_equipped_name.take();
            draft.equipped.insert(slot, item);
        } else if key == "level" {
            set_once(
                &mut draft.level,
                parse_number(value, line_number, key)?,
                line_number,
                key,
            )?;
        } else if key == "race" {
            set_once(
                &mut draft.race,
                safe_token(value, line_number, key)?,
                line_number,
                key,
            )?;
        } else if key == "region" {
            set_once(
                &mut draft.region,
                safe_token(value, line_number, key)?,
                line_number,
                key,
            )?;
        } else if key == "server" {
            set_once(
                &mut draft.server,
                safe_token(value, line_number, key)?,
                line_number,
                key,
            )?;
        } else if key == "role" {
            let role = match value {
                "attack" | "melee" | "spell" | "hybrid" | "dps" => Some(Role::Attack),
                "heal" => Some(Role::Heal),
                "tank" => Some(Role::Tank),
                "auto" => None,
                _ => return invalid(line_number, format!("unsupported role: {value}")),
            };
            set_once(&mut draft.role, role, line_number, key)?;
        } else if key == "spec" {
            set_once(
                &mut draft.specialization,
                safe_token(value, line_number, key)?,
                line_number,
                key,
            )?;
        } else if is_simulation_option(key) {
            parse_simulation_option(&mut draft, key, value, line_number)?;
        } else if is_talent_key(key) {
            insert_unique(&mut draft.talents, key, value, line_number)?;
        } else if key.starts_with("actions") {
            if value.is_empty() {
                return invalid(line_number, "action directive cannot be empty");
            }
            draft.actions.push(ActionDirective {
                key: key.to_owned(),
                value: value.to_owned(),
            });
        } else if is_scalar_option(key) {
            insert_unique(&mut draft.scalar_options, key, value, line_number)?;
        }
    }

    if let (Some(addon_version), Some((wow_version, wow_build, toc))) =
        (addon_version, wow_metadata)
    {
        validate_retail_metadata(&wow_version, toc)?;
        draft.addon = Some(AddonMetadata {
            addon_version,
            wow_version,
            wow_build,
            toc,
        });
    } else if source_kind == SourceKind::AddonExport {
        return Err(ParseError::Missing("SimC AddOn and WoW metadata headers"));
    }

    let channel = if draft.addon.is_some() {
        GameChannel::RetailLive
    } else {
        GameChannel::Unspecified
    };
    let specialization = draft
        .specialization
        .ok_or(ParseError::Missing("specialization"))?;
    let role = draft
        .role
        .flatten()
        .unwrap_or_else(|| inferred_role(&specialization));
    Ok(Profile {
        source_kind,
        channel,
        addon: draft.addon,
        class: draft.class.ok_or(ParseError::Missing("player class"))?,
        name: draft.name.ok_or(ParseError::Missing("player name"))?,
        level: draft.level.ok_or(ParseError::Missing("level"))?,
        race: draft.race.ok_or(ParseError::Missing("race"))?,
        region: draft.region,
        server: draft.server,
        role,
        specialization,
        scalar_options: draft.scalar_options,
        talents: draft.talents,
        saved_talent_loadouts: draft.saved_talent_loadouts,
        equipped: draft.equipped,
        bag_items: draft.bag_items,
        upgrade_metadata: (!draft.upgrade_metadata.raw_fields.is_empty())
            .then_some(draft.upgrade_metadata),
        actions: draft.actions,
        simulation: draft.simulation,
    })
}

fn project_upgrade_metadata(
    metadata: &mut UpgradeMetadata,
    key: &str,
    value: &str,
    line: usize,
) -> Result<()> {
    if metadata.raw_fields.contains_key(key) {
        return invalid(line, format!("duplicate {key} metadata"));
    }
    metadata.raw_fields.insert(key.to_owned(), value.to_owned());
    metadata.source_lines.insert(key.to_owned(), line);
    match key {
        "upgrade_currencies" => {
            for entry in value.split('/').filter(|entry| !entry.is_empty()) {
                let mut parts = entry.split(':');
                let kind = parts.next().unwrap_or_default();
                let id = parts.next().unwrap_or_default();
                let amount = parts.next().unwrap_or_default();
                if !matches!(kind, "c" | "i")
                    || id.parse::<u32>().is_err()
                    || parts.next().is_some()
                {
                    return invalid(line, "invalid upgrade_currencies metadata");
                }
                metadata.currencies.insert(
                    format!("{kind}:{id}"),
                    parse_number(amount, line, "upgrade currency amount")?,
                );
            }
        }
        "upgrade_achievements" => {
            metadata.achievements = value
                .split('/')
                .filter(|entry| !entry.is_empty())
                .map(|entry| parse_number(entry, line, "upgrade achievement"))
                .collect::<Result<Vec<_>>>()?;
        }
        "slot_high_watermarks" => {
            for entry in value.split('/').filter(|entry| !entry.is_empty()) {
                let parts = entry.split(':').collect::<Vec<_>>();
                if parts.len() != 3 {
                    return invalid(line, "invalid slot_high_watermarks metadata");
                }
                let slot_index = parse_number(parts[0], line, "slot watermark index")?;
                metadata.slot_high_watermarks.insert(
                    slot_index,
                    SlotHighWatermark {
                        slot_index,
                        character_item_level: parse_number(
                            parts[1],
                            line,
                            "character slot high watermark",
                        )?,
                        account_item_level: parse_number(
                            parts[2],
                            line,
                            "account slot high watermark",
                        )?,
                    },
                );
            }
        }
        _ => unreachable!("caller filters metadata keys"),
    }
    Ok(())
}

fn looks_like_item_comment(comment: &str) -> bool {
    comment
        .rsplit_once(" (")
        .and_then(|(_, suffix)| suffix.strip_suffix(')'))
        .is_some_and(|item_level| {
            !item_level.is_empty() && item_level.chars().all(|c| c.is_ascii_digit())
        })
}

fn item_name_from_comment(comment: &str) -> String {
    comment
        .rsplit_once(" (")
        .map_or(comment, |(name, _)| name)
        .to_owned()
}

fn contains_channel_marker(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "game_channel=classic",
        "game_channel=ptr",
        "game_channel=beta",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || lower.starts_with("# wow classic")
        || lower.starts_with("# wow ptr")
        || lower.starts_with("# wow beta")
}

fn validate_retail_metadata(version: &str, toc: u32) -> Result<()> {
    let major = version
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_default();
    if !(10..=19).contains(&major) || !(100_000..200_000).contains(&toc) {
        return invalid(1, "AddOn metadata is not from Retail Live");
    }
    Ok(())
}

pub(crate) fn parse_class(key: &str) -> Option<CharacterClass> {
    Some(match key {
        "deathknight" | "death_knight" => CharacterClass::DeathKnight,
        "demonhunter" | "demon_hunter" => CharacterClass::DemonHunter,
        "druid" => CharacterClass::Druid,
        "evoker" => CharacterClass::Evoker,
        "hunter" => CharacterClass::Hunter,
        "mage" => CharacterClass::Mage,
        "monk" => CharacterClass::Monk,
        "paladin" => CharacterClass::Paladin,
        "priest" => CharacterClass::Priest,
        "rogue" => CharacterClass::Rogue,
        "shaman" => CharacterClass::Shaman,
        "warlock" => CharacterClass::Warlock,
        "warrior" => CharacterClass::Warrior,
        _ => return None,
    })
}

pub(crate) fn parse_gear_slot(key: &str) -> Option<GearSlot> {
    Some(match key {
        "head" => GearSlot::Head,
        "neck" => GearSlot::Neck,
        "shoulder" | "shoulders" => GearSlot::Shoulders,
        "back" => GearSlot::Back,
        "chest" => GearSlot::Chest,
        "shirt" => GearSlot::Shirt,
        "tabard" => GearSlot::Tabard,
        "wrist" | "wrists" => GearSlot::Wrists,
        "hand" | "hands" => GearSlot::Hands,
        "waist" => GearSlot::Waist,
        "leg" | "legs" => GearSlot::Legs,
        "foot" | "feet" => GearSlot::Feet,
        "finger1" | "ring1" => GearSlot::Finger1,
        "finger2" | "ring2" => GearSlot::Finger2,
        "trinket1" => GearSlot::Trinket1,
        "trinket2" => GearSlot::Trinket2,
        "main_hand" => GearSlot::MainHand,
        "off_hand" => GearSlot::OffHand,
        _ => return None,
    })
}

fn parse_item(slot: GearSlot, value: &str, line: usize) -> Result<Item> {
    let mut id = None;
    let mut options = BTreeMap::new();
    for (index, part) in value.split(',').map(str::trim).enumerate() {
        if part.is_empty() {
            continue;
        }
        if index == 0 && !part.contains('=') {
            validate_item_name(part, line)?;
            continue;
        }
        let (key, value) = part.split_once('=').ok_or_else(|| ParseError::Invalid {
            line,
            message: format!("invalid item option: {part}"),
        })?;
        validate_key(key, line)?;
        let value = safe_option_value(value, line, key)?;
        if key == "id" {
            let parsed = parse_number::<u32>(&value, line, "item id")?;
            if parsed == 0 || id.replace(parsed).is_some() {
                return invalid(line, "item must contain one non-zero id");
            }
        } else if key == "context" {
            let _ = parse_number::<u32>(&value, line, "item context")?;
            if options.insert(key.to_owned(), value).is_some() {
                return invalid(line, format!("duplicate item option: {key}"));
            }
        } else if options.insert(key.to_owned(), value).is_some() {
            return invalid(line, format!("duplicate item option: {key}"));
        }
    }
    Ok(Item {
        slot,
        id: id.ok_or_else(|| ParseError::Invalid {
            line,
            message: "item id is required".into(),
        })?,
        name: None,
        options,
    })
}

fn validate_item_name(value: &str, line: usize) -> Result<()> {
    if value.len() > 128
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '\'')
        })
    {
        return invalid(line, "item name is not a safe SimulationCraft token");
    }
    Ok(())
}

fn inferred_role(specialization: &str) -> Role {
    if matches!(
        specialization,
        "blood" | "vengeance" | "guardian" | "brewmaster" | "protection"
    ) {
        Role::Tank
    } else if matches!(
        specialization,
        "restoration" | "preservation" | "mistweaver" | "holy" | "discipline"
    ) {
        Role::Heal
    } else {
        Role::Attack
    }
}

fn is_simulation_option(key: &str) -> bool {
    matches!(
        key,
        "iterations"
            | "fixed_time"
            | "max_time"
            | "vary_combat_length"
            | "desired_targets"
            | "fight_style"
            | "threads"
            | "seed"
            | "report_details"
    )
}

fn parse_simulation_option(
    draft: &mut ProfileDraft,
    key: &str,
    value: &str,
    line: usize,
) -> Result<()> {
    match key {
        "iterations" => {
            draft.simulation.iterations = bounded_number(value, line, key, 1, 10_000_000)?
        }
        "fixed_time" => draft.simulation.fixed_time = parse_bool(value, line, key)?,
        "max_time" => {
            draft.simulation.max_time_seconds = bounded_number(value, line, key, 1, 3_600)?
        }
        "vary_combat_length" => {
            let parsed = parse_number::<f64>(value, line, key)?;
            if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
                return invalid(line, "vary_combat_length must be between 0 and 1");
            }
            draft.simulation.vary_combat_length = parsed;
        }
        "desired_targets" => {
            draft.simulation.desired_targets = bounded_number(value, line, key, 1, 40)?
        }
        "fight_style" => {
            if !matches!(
                value,
                "Patchwerk"
                    | "CastingPatchwerk"
                    | "HecticAddCleave"
                    | "DungeonSlice"
                    | "LightMovement"
                    | "HeavyMovement"
                    | "HelterSkelter"
                    | "CleaveAdd"
                    | "Beastlord"
            ) {
                return invalid(line, format!("unsupported fight_style: {value}"));
            }
            draft.simulation.fight_style = value.to_owned();
        }
        "threads" => draft.simulation.threads = bounded_number(value, line, key, 1, 256)?,
        "seed" => draft.simulation.seed = bounded_number(value, line, key, 1, u64::MAX)?,
        "report_details" => draft.simulation.report_details = parse_bool(value, line, key)?,
        _ => unreachable!("caller checked simulation option"),
    }
    Ok(())
}

pub(crate) fn is_talent_key(key: &str) -> bool {
    matches!(
        key,
        "talents" | "class_talents" | "spec_talents" | "hero_talents" | "omnium_talents"
    )
}

pub(crate) fn is_scalar_option(key: &str) -> bool {
    matches!(
        key,
        "position"
            | "professions"
            | "loot_spec"
            | "zandalari_loa"
            | "flask"
            | "food"
            | "augmentation"
            | "potion"
            | "temporary_enchant"
            | "load_default_gear"
            | "source"
            | "default_actions"
            | "timeofday"
            | "warlock.default_pet"
    )
}

fn parse_player_name(value: &str, line: usize) -> Result<String> {
    let value = if value.starts_with('"') || value.ends_with('"') {
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or_else(|| ParseError::Invalid {
                line,
                message: "player name has unmatched quotes".into(),
            })?
    } else {
        value
    };
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '"' | '\\'))
    {
        return invalid(line, "player name contains unsupported characters");
    }
    Ok(value.to_owned())
}

fn safe_token(value: &str, line: usize, label: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return invalid(line, format!("{label} is not a safe token"));
    }
    Ok(value.to_owned())
}

fn safe_option_value(value: &str, line: usize, label: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 4096
        || !value.chars().all(|character| {
            character.is_alphanumeric()
                || matches!(character, '_' | '-' | '.' | '/' | ':' | '+' | '%' | '@')
                || character == '='
        })
    {
        return invalid(line, format!("{label} contains unsupported characters"));
    }
    Ok(value.to_owned())
}

fn validate_key(key: &str, line: usize) -> Result<()> {
    let base = key.strip_suffix('+').unwrap_or(key);
    if base.is_empty()
        || !base.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '.'
        })
    {
        return invalid(line, format!("invalid option key: {key}"));
    }
    Ok(())
}

fn insert_unique(
    target: &mut BTreeMap<String, String>,
    key: &str,
    value: &str,
    line: usize,
) -> Result<()> {
    let value = safe_option_value(value, line, key)?;
    target.insert(key.to_owned(), value);
    Ok(())
}

fn set_once<T>(target: &mut Option<T>, value: T, line: usize, label: &str) -> Result<()> {
    let _ = (line, label);
    target.replace(value);
    Ok(())
}

fn parse_bool(value: &str, line: usize, label: &str) -> Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => invalid(line, format!("{label} must be 0 or 1")),
    }
}

fn bounded_number<T>(value: &str, line: usize, label: &str, min: T, max: T) -> Result<T>
where
    T: FromStr + PartialOrd + Copy,
{
    let parsed = parse_number(value, line, label)?;
    if parsed < min || parsed > max {
        return invalid(line, format!("{label} is outside the supported range"));
    }
    Ok(parsed)
}

fn parse_number<T>(value: &str, line: usize, label: &str) -> Result<T>
where
    T: FromStr,
{
    value.parse::<T>().map_err(|_| ParseError::Invalid {
        line,
        message: format!("{label} is not a valid number"),
    })
}

fn invalid<T>(line: usize, message: impl Into<String>) -> Result<T> {
    Err(ParseError::Invalid {
        line,
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_output_and_channel_overrides_at_execution_policy() {
        let base = "warrior=Test\nlevel=90\nrace=orc\nrole=attack\nspec=fury\n";
        for option in ["ptr=1", "json2=/tmp/out.json", "profileset.one=max_time=1"] {
            let error = parse_simc_file(&format!("{base}{option}\n")).unwrap_err();
            assert!(
                error.to_string().contains("controlled")
                    || error.to_string().contains("unsupported")
                    || error.to_string().contains("not executable")
            );
        }
    }

    #[test]
    fn rejects_multiple_actors_but_preserves_unknown_options() {
        let error = parse_simc_file(
            "warrior=One\nwarrior=Two\nlevel=90\nrace=orc\nrole=attack\nspec=fury\n",
        )
        .unwrap_err();
        assert!(error.to_string().contains("multiple player actors"));

        let source = "warrior=One\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nunknown=1\n";
        assert!(parse_simc_file(source).is_ok());
        let report = analyze_compatibility(&parse_document(source).unwrap());
        assert_eq!(report.preserved_not_editable, 1);
    }

    #[test]
    fn accepts_current_addon_trait_and_profession_shapes() {
        let profile = parse_addon_export(
            "# SimC Addon 12.1.0-01\n# WoW 12.1.0.69465, TOC 120100\nwarrior=One\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nprofessions=alchemy=100/blacksmithing=100\nomnium_talents=123:1/456:2\n",
        )
        .unwrap();
        assert_eq!(
            profile.scalar_options["professions"],
            "alchemy=100/blacksmithing=100"
        );
        assert_eq!(profile.talents["omnium_talents"], "123:1/456:2");
    }

    #[test]
    fn accepts_simc_dps_role_aliases() {
        for role in ["melee", "spell", "hybrid", "dps"] {
            let profile = parse_simc_file(&format!(
                "rogue=Character\nlevel=90\nrace=void_elf\nregion=kr\nserver=azshara\nrole={role}\nspec=subtlety\ntalents=CUQAAAAAAAA\nhead=,id=250006,bonus_id=6652/12667,context=35\nshoulder=,id=250004,context=35\n"
            ))
            .unwrap();
            assert_eq!(profile.role, Role::Attack);
            assert_eq!(profile.equipped[&GearSlot::Head].options["context"], "35");
            assert_eq!(profile.equipped[&GearSlot::Shoulders].id, 250004);
        }
    }

    #[test]
    fn projects_official_addon_item_comments_and_saved_talent_loadouts() {
        let profile = parse_simc_file(
            "rogue=Character\nlevel=90\nrace=void_elf\nrole=melee\nspec=subtlety\ntalents=ACTIVE\n\n# Saved Loadout: Dungeon\n# talents=SAVED\n\n# Masquerade of the Grim Jest (289)\nhead=,id=250006,context=35\n",
        )
        .unwrap();
        assert_eq!(
            profile.equipped[&GearSlot::Head].name.as_deref(),
            Some("Masquerade of the Grim Jest")
        );
        assert_eq!(
            profile.saved_talent_loadouts,
            vec![TalentLoadout {
                name: "Dungeon".into(),
                value: "SAVED".into(),
            }]
        );
    }

    #[test]
    fn projects_upgrade_metadata_without_changing_the_lossless_document() {
        let source = "# SimC Addon 12.1.0-1\n# WoW 12.1.0.69465, TOC 120100\nrogue=Tester\nlevel=90\nrace=human\nrole=attack\nspec=subtlety\n# upgrade_currencies=c:1792:2206/i:204195:4\n# upgrade_achievements=40943/42768\n# slot_high_watermarks=0:415:418/12:398:402\n";
        let document = parse_document(source).unwrap();
        assert_eq!(document.as_bytes(), source.as_bytes());
        let profile = project_profile(&document, SourceKind::AddonExport).unwrap();
        let metadata = profile.upgrade_metadata.unwrap();
        assert_eq!(metadata.currencies["c:1792"], 2206);
        assert_eq!(metadata.currencies["i:204195"], 4);
        assert_eq!(metadata.achievements, vec![40943, 42768]);
        assert_eq!(metadata.slot_high_watermarks[&0].account_item_level, 418);
        assert_eq!(metadata.source_lines["upgrade_currencies"], 8);
    }

    #[test]
    fn accepts_the_official_simc_profile_shape_and_aliases() {
        let profile = parse_simc_file(
            "deathknight=Official\nlevel=90\nrace=void_elf\nrole=auto\nspec=blood\ntimeofday=night\nshoulders=sample_shoulders,id=250004,ilevel=289,embellishment=sample\nwrists=sample_bracers,id=244576,crafted_stats=32/49,enchant=sample\nring1=sample_ring,id=251217,gem_id=240908\n",
        )
        .unwrap();

        assert_eq!(profile.class, CharacterClass::DeathKnight);
        assert_eq!(profile.role, Role::Tank);
        assert_eq!(profile.scalar_options["timeofday"], "night");
        assert_eq!(profile.equipped[&GearSlot::Shoulders].id, 250004);
        assert_eq!(
            profile.equipped[&GearSlot::Wrists].options["enchant"],
            "sample"
        );
        assert_eq!(profile.equipped[&GearSlot::Finger1].id, 251217);
    }

    #[test]
    fn infers_role_when_the_official_optional_field_is_absent() {
        let healer = parse_simc_file(
            "priest=Healer\nlevel=90\nrace=human\nspec=discipline\nload_default_gear=1\n",
        )
        .unwrap();
        let damage = parse_simc_file(
            "mage=Damage\nlevel=90\nrace=human\nspec=arcane\nload_default_gear=1\n",
        )
        .unwrap();

        assert_eq!(healer.role, Role::Heal);
        assert_eq!(damage.role, Role::Attack);
    }

    #[test]
    fn rejects_classic_ptr_and_beta_inputs() {
        let classic = "# SimC Addon 1.15.7-01\n# WoW 1.15.7.60000, TOC 11507\nwarrior=One\nlevel=60\nrace=orc\nrole=attack\nspec=fury\n";
        assert!(parse_addon_export(classic).is_err());
        for directive in ["ptr=1", "beta=1", "classic=1"] {
            let source =
                format!("warrior=One\nlevel=90\nrace=orc\nrole=attack\nspec=fury\n{directive}\n");
            assert!(parse_simc_file(&source).is_err());
        }
    }

    #[test]
    fn applies_sequential_last_value_semantics_to_typed_projection() {
        let profile = parse_simc_file(
            "warrior=One\nlevel=80\nlevel=90\nrace=orc\nrole=attack\nspec=fury\niterations=100\niterations=200\ntalents=OLD\ntalents=NEW\nhead=,id=1\nhead=,id=2\n",
        )
        .unwrap();
        assert_eq!(profile.level, 90);
        assert_eq!(profile.simulation.iterations, 200);
        assert_eq!(profile.talents["talents"], "NEW");
        assert_eq!(profile.equipped[&GearSlot::Head].id, 2);
    }
}

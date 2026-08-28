use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use simshredder_domain::GearSlot;
use thiserror::Error;

pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const MAX_ENTRIES: usize = 8_192;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnhancementKind {
    Gem,
    Enchant,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnhancementEntry {
    pub kind: EnhancementKind,
    /// ID written to SimC's `gem_id` or `enchant_id` option.
    pub input_id: u32,
    /// Optional in-game consumable item that applies this effect.
    pub source_item_id: Option<u32>,
    pub spell_id: Option<u32>,
    /// Locale keys use Blizzard locale names such as `en_US` and `ko_KR`.
    pub names: BTreeMap<String, String>,
    /// Enchant applicability. Gems use `socket_types` instead.
    pub slots: BTreeSet<GearSlot>,
    pub socket_types: BTreeSet<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnhancementCatalog {
    pub schema_version: u32,
    pub revision: String,
    pub game_version: String,
    pub game_build: u32,
    pub season: String,
    pub generated_at: String,
    pub source: String,
    pub entries: Vec<EnhancementEntry>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("enhancement catalog JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("enhancement catalog is invalid: {0}")]
    Invalid(String),
    #[error("enhancement catalog targets build {catalog_build}, not {requested_build}")]
    BuildMismatch {
        catalog_build: u32,
        requested_build: u32,
    },
    #[error("no enhancement catalog is available for game build {0}")]
    MissingBuild(u32),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Keeps the optimizer independent from bundled, downloaded, or test catalog storage.
pub trait EnhancementCatalogProvider {
    fn catalog_for_build(&self, game_build: u32) -> Result<EnhancementCatalog>;
}

pub struct StaticJsonCatalogProvider<'a> {
    documents: &'a [&'a [u8]],
}

impl<'a> StaticJsonCatalogProvider<'a> {
    pub const fn new(documents: &'a [&'a [u8]]) -> Self {
        Self { documents }
    }
}

impl EnhancementCatalogProvider for StaticJsonCatalogProvider<'_> {
    fn catalog_for_build(&self, game_build: u32) -> Result<EnhancementCatalog> {
        for document in self.documents {
            let catalog = EnhancementCatalog::from_json(document)?;
            if catalog.game_build == game_build {
                return Ok(catalog);
            }
        }
        Err(Error::MissingBuild(game_build))
    }
}

impl EnhancementCatalog {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let catalog: Self = serde_json::from_slice(bytes)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn for_build(bytes: &[u8], requested_build: u32) -> Result<Self> {
        let catalog = Self::from_json(bytes)?;
        if catalog.game_build != requested_build {
            return Err(Error::BuildMismatch {
                catalog_build: catalog.game_build,
                requested_build,
            });
        }
        Ok(catalog)
    }

    pub fn gems(&self) -> impl Iterator<Item = &EnhancementEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == EnhancementKind::Gem)
    }

    pub fn enchants_for(&self, slot: GearSlot) -> impl Iterator<Item = &EnhancementEntry> {
        self.entries.iter().filter(move |entry| {
            entry.kind == EnhancementKind::Enchant && entry.slots.contains(&slot)
        })
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "unsupported schema version {}",
                self.schema_version
            )));
        }
        if self.game_build == 0
            || !bounded_text(&self.revision, 128)
            || !bounded_text(&self.game_version, 64)
            || !bounded_text(&self.season, 128)
            || !bounded_text(&self.generated_at, 64)
            || !bounded_text(&self.source, 512)
        {
            return Err(Error::Invalid("catalog metadata is incomplete".into()));
        }
        if self.entries.len() > MAX_ENTRIES {
            return Err(Error::Invalid(format!("entry count exceeds {MAX_ENTRIES}")));
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            if entry.input_id == 0
                || !identities.insert((entry.kind, entry.input_id))
                || entry.source_item_id == Some(0)
                || entry.spell_id == Some(0)
            {
                return Err(Error::Invalid(
                    "entry IDs must be positive and unique per kind".into(),
                ));
            }
            if entry.names.len() > 32
                || entry
                    .names
                    .iter()
                    .any(|(locale, name)| !valid_locale(locale) || !bounded_text(name, 256))
            {
                return Err(Error::Invalid("localized names are invalid".into()));
            }
            if entry.socket_types.len() > 16
                || entry.tags.len() > 32
                || entry
                    .socket_types
                    .iter()
                    .chain(entry.tags.iter())
                    .any(|value| !safe_token(value))
            {
                return Err(Error::Invalid("entry tags are invalid".into()));
            }
            match entry.kind {
                EnhancementKind::Gem if !entry.slots.is_empty() => {
                    return Err(Error::Invalid(
                        "gem entries cannot target gear slots".into(),
                    ));
                }
                EnhancementKind::Enchant
                    if entry.slots.is_empty() || !entry.socket_types.is_empty() =>
                {
                    return Err(Error::Invalid(
                        "enchant entries require slots and cannot use socket types".into(),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.contains(['\r', '\n', '\0'])
}

fn valid_locale(value: &str) -> bool {
    value.len() == 5
        && value.as_bytes()[2] == b'_'
        && value[..2].bytes().all(|byte| byte.is_ascii_lowercase())
        && value[3..].bytes().all(|byte| byte.is_ascii_uppercase())
}

fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const EMPTY_0X: &[u8] =
        include_bytes!("../../../../resources/game-data/enhancements/12.1.0-69465-empty-v1.json");

    #[test]
    fn bundled_0x_catalog_is_explicitly_empty_and_build_scoped() {
        let documents: &[&[u8]] = &[EMPTY_0X];
        let provider = StaticJsonCatalogProvider::new(documents);
        let catalog = provider.catalog_for_build(69465).unwrap();
        assert_eq!(catalog.revision, "12.1.0-69465-empty-v1");
        assert!(catalog.entries.is_empty());
        assert!(matches!(
            provider.catalog_for_build(69497),
            Err(Error::MissingBuild(69497))
        ));
    }

    #[test]
    fn indexes_gems_and_slot_specific_enchants_without_mixing_ids() {
        let catalog = EnhancementCatalog {
            schema_version: 1,
            revision: "fixture-v1".into(),
            game_version: "12.1.0".into(),
            game_build: 69465,
            season: "Midnight Season 2".into(),
            generated_at: "2026-08-28T00:00:00Z".into(),
            source: "test fixture".into(),
            entries: vec![
                EnhancementEntry {
                    kind: EnhancementKind::Gem,
                    input_id: 240918,
                    source_item_id: Some(240918),
                    spell_id: None,
                    names: BTreeMap::from([("en_US".into(), "Fixture Gem".into())]),
                    slots: BTreeSet::new(),
                    socket_types: BTreeSet::from(["prismatic".into()]),
                    tags: BTreeSet::new(),
                },
                EnhancementEntry {
                    kind: EnhancementKind::Enchant,
                    input_id: 8017,
                    source_item_id: None,
                    spell_id: Some(999),
                    names: BTreeMap::from([("ko_KR".into(), "시험 마법부여".into())]),
                    slots: BTreeSet::from([GearSlot::Head]),
                    socket_types: BTreeSet::new(),
                    tags: BTreeSet::from(["avoidance".into()]),
                },
            ],
        };
        catalog.validate().unwrap();
        assert_eq!(catalog.gems().count(), 1);
        assert_eq!(catalog.enchants_for(GearSlot::Head).count(), 1);
        assert_eq!(catalog.enchants_for(GearSlot::Chest).count(), 0);
    }

    #[test]
    fn rejects_duplicate_and_cross_kind_shape_errors() {
        let mut entry = EnhancementEntry {
            kind: EnhancementKind::Enchant,
            input_id: 42,
            source_item_id: None,
            spell_id: None,
            names: BTreeMap::new(),
            slots: BTreeSet::from([GearSlot::Chest]),
            socket_types: BTreeSet::new(),
            tags: BTreeSet::new(),
        };
        let mut catalog = EnhancementCatalog {
            schema_version: 1,
            revision: "fixture-v1".into(),
            game_version: "12.1.0".into(),
            game_build: 69465,
            season: "Season".into(),
            generated_at: "2026-08-28T00:00:00Z".into(),
            source: "fixture".into(),
            entries: vec![entry.clone(), entry.clone()],
        };
        assert!(catalog.validate().is_err());
        entry.kind = EnhancementKind::Gem;
        catalog.entries = vec![entry];
        assert!(catalog.validate().is_err());
    }
}

//! Pure, serializable domain types shared by the headless SimShredder core.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterClass {
    DeathKnight,
    DemonHunter,
    Druid,
    Evoker,
    Hunter,
    Mage,
    Monk,
    Paladin,
    Priest,
    Rogue,
    Shaman,
    Warlock,
    Warrior,
}

impl CharacterClass {
    pub const fn simc_token(self) -> &'static str {
        match self {
            Self::DeathKnight => "death_knight",
            Self::DemonHunter => "demon_hunter",
            Self::Druid => "druid",
            Self::Evoker => "evoker",
            Self::Hunter => "hunter",
            Self::Mage => "mage",
            Self::Monk => "monk",
            Self::Paladin => "paladin",
            Self::Priest => "priest",
            Self::Rogue => "rogue",
            Self::Shaman => "shaman",
            Self::Warlock => "warlock",
            Self::Warrior => "warrior",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Attack,
    Heal,
    Tank,
}

impl Role {
    pub const fn simc_token(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Heal => "heal",
            Self::Tank => "tank",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameChannel {
    RetailLive,
    Unspecified,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearSlot {
    Head,
    Neck,
    Shoulders,
    Back,
    Chest,
    Shirt,
    Tabard,
    Wrists,
    Hands,
    Waist,
    Legs,
    Feet,
    Finger1,
    Finger2,
    Trinket1,
    Trinket2,
    MainHand,
    OffHand,
}

impl GearSlot {
    pub const fn simc_token(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Neck => "neck",
            Self::Shoulders => "shoulders",
            Self::Back => "back",
            Self::Chest => "chest",
            Self::Shirt => "shirt",
            Self::Tabard => "tabard",
            Self::Wrists => "wrists",
            Self::Hands => "hands",
            Self::Waist => "waist",
            Self::Legs => "legs",
            Self::Feet => "feet",
            Self::Finger1 => "finger1",
            Self::Finger2 => "finger2",
            Self::Trinket1 => "trinket1",
            Self::Trinket2 => "trinket2",
            Self::MainHand => "main_hand",
            Self::OffHand => "off_hand",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub slot: GearSlot,
    pub id: u32,
    pub options: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BagItem {
    pub item: Item,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimulationOptions {
    pub iterations: u32,
    pub fixed_time: bool,
    pub max_time_seconds: u32,
    pub vary_combat_length: f64,
    pub desired_targets: u16,
    pub fight_style: String,
    pub threads: u16,
    pub seed: u64,
    pub report_details: bool,
}

impl Default for SimulationOptions {
    fn default() -> Self {
        Self {
            iterations: 10_000,
            fixed_time: false,
            max_time_seconds: 300,
            vary_combat_length: 0.2,
            desired_targets: 1,
            fight_style: "Patchwerk".into(),
            threads: 1,
            seed: 1,
            report_details: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActionDirective {
    /// Includes the action list and optional `+`, for example `actions.precombat+`.
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub source_kind: SourceKind,
    pub channel: GameChannel,
    pub addon: Option<AddonMetadata>,
    pub class: CharacterClass,
    pub name: String,
    pub level: u16,
    pub race: String,
    pub region: Option<String>,
    pub server: Option<String>,
    pub role: Role,
    pub specialization: String,
    pub scalar_options: BTreeMap<String, String>,
    pub talents: BTreeMap<String, String>,
    pub equipped: BTreeMap<GearSlot, Item>,
    pub bag_items: Vec<BagItem>,
    pub actions: Vec<ActionDirective>,
    pub simulation: SimulationOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    AddonExport,
    SimcFile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AddonMetadata {
    pub addon_version: String,
    pub wow_version: String,
    pub wow_build: u32,
    pub toc: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormalizedQuickResult {
    pub schema_version: u32,
    pub report_version: String,
    pub runtime: ResultRuntimeIdentity,
    pub player: ResultPlayer,
    pub options: ResultOptions,
    pub primary_metric: StatisticalMetric,
    pub actions: Vec<ResultAction>,
    pub buffs: Vec<ResultBuff>,
    pub resources: Vec<ResultResource>,
    pub apl_sequence: Vec<ResultAplAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultAction {
    pub id: Option<u32>,
    pub name: String,
    pub internal_name: String,
    pub school: String,
    pub executes: f64,
    pub amount_per_fight: f64,
    pub metric_per_second: f64,
    pub share: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultBuff {
    pub id: Option<u32>,
    pub name: String,
    pub internal_name: String,
    pub uptime_percent: f64,
    pub benefit_percent: Option<f64>,
    pub starts: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultResource {
    pub name: String,
    pub spent_per_fight: f64,
    pub overflow_per_fight: f64,
    pub remaining_per_fight: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultAplAction {
    pub time_seconds: f64,
    pub id: Option<u32>,
    pub name: String,
    pub internal_name: String,
    pub target: String,
    pub resources: BTreeMap<String, f64>,
    pub resource_max: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResultRuntimeIdentity {
    pub simc_version: String,
    pub git_revision: String,
    pub game_version: String,
    pub game_build: u32,
    pub channel: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResultPlayer {
    pub name: String,
    pub race: String,
    pub role: String,
    pub specialization: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultOptions {
    pub iterations: u64,
    pub threads: u64,
    pub seed: u64,
    pub max_time_seconds: f64,
    pub desired_targets: u64,
    pub fight_style: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatisticalMetric {
    pub name: String,
    pub mean: f64,
    pub mean_error: f64,
    pub standard_deviation: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub median: f64,
}

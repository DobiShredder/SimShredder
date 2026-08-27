//! Deterministic Top Gear search, SimulationCraft profileset I/O, ranking, and action planning.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use simshredder_domain::GearSlot;
use thiserror::Error;

pub type CostVector = BTreeMap<String, u32>;

const MAX_RAW_COMBINATIONS: u64 = 2_000_000;
const MAX_EMITTED_COMBINATIONS: usize = 10_000;
const MAX_CANDIDATES_PER_SLOT: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuleManifest {
    pub schema_version: u32,
    pub revision: String,
    pub game_build: u32,
    pub game_version: String,
    pub source: String,
    pub currency_ids: BTreeSet<String>,
    pub max_embellishments: u8,
    pub max_sockets_per_item: u8,
    pub allowed_enchant_slots: BTreeSet<GearSlot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    None,
    OneHand,
    TwoHand,
    OffHand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Equip,
    Gem,
    Enchant,
    Upgrade,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeAction {
    pub id: String,
    pub label: String,
    pub kind: ChangeKind,
    pub cost: CostVector,
    pub depends_on: Vec<String>,
    pub from_rank: Option<u8>,
    pub to_rank: Option<u8>,
    pub slot: GearSlot,
    pub source_item_id: u32,
    pub simc_options_patch: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemVariant {
    pub key: String,
    pub source_item_id: u32,
    pub slot: GearSlot,
    pub rank: u8,
    pub gem_ids: Vec<u32>,
    pub enchant_id: Option<u32>,
    pub simc_options: BTreeMap<String, String>,
    pub cost: CostVector,
    pub actions: Vec<UpgradeAction>,
    pub unique_groups: BTreeSet<String>,
    pub weapon_kind: WeaponKind,
    pub embellishment: bool,
    pub changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSnapshot {
    pub balances: CostVector,
    pub reserves: CostVector,
    pub confirmed_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub expected_rule_revision: String,
    pub game_build: u32,
    pub candidates: BTreeMap<GearSlot, Vec<ItemVariant>>,
    pub budget: BudgetSnapshot,
    pub max_combinations: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Loadout {
    pub key: String,
    pub items: BTreeMap<GearSlot, ItemVariant>,
    pub cost: CostVector,
    pub changed_slots: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPreview {
    pub raw_combinations: u64,
    pub valid_combinations: u64,
    pub emitted_combinations: usize,
    pub was_truncated: bool,
    pub rejections: RejectionBreakdown,
    pub loadouts: Vec<Loadout>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectionBreakdown {
    /// Candidate variants removed because an identical final SimC tuple costs more.
    pub dominated_variants: u64,
    pub socket_limit: u64,
    pub enchant_slot: u64,
    pub unique_equipped: u64,
    pub embellishment_limit: u64,
    pub weapon_constraint: u64,
    pub budget: u64,
    pub symmetric_duplicate: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RejectionReason {
    SocketLimit,
    EnchantSlot,
    UniqueEquipped,
    EmbellishmentLimit,
    WeaponConstraint,
    Budget,
}

impl RejectionBreakdown {
    fn record(&mut self, reason: RejectionReason) {
        let counter = match reason {
            RejectionReason::SocketLimit => &mut self.socket_limit,
            RejectionReason::EnchantSlot => &mut self.enchant_slot,
            RejectionReason::UniqueEquipped => &mut self.unique_equipped,
            RejectionReason::EmbellishmentLimit => &mut self.embellishment_limit,
            RejectionReason::WeaponConstraint => &mut self.weapon_constraint,
            RejectionReason::Budget => &mut self.budget,
        };
        *counter = counter.saturating_add(1);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedLoadout {
    pub loadout: Loadout,
    pub mean: f64,
    pub mean_error: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedLoadout {
    pub loadout: Loadout,
    pub mean: f64,
    pub mean_error: f64,
    pub delta: f64,
    pub combined_error: f64,
    pub equivalent_to_baseline: bool,
    pub pareto_optimal: bool,
    pub rank: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedAction {
    pub id: String,
    pub label: String,
    pub kind: ChangeKind,
    pub cost: CostVector,
    pub remaining: CostVector,
    pub marginal_gain: f64,
    pub cumulative_gain: f64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionState {
    pub loadout: Loadout,
    pub applied_action_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesetResult {
    pub key: String,
    pub mean: f64,
    pub mean_error: f64,
    pub iterations: u64,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("rule revision mismatch: expected {expected}, loaded {loaded}")]
    RuleRevision { expected: String, loaded: String },
    #[error("game build mismatch: expected {expected}, loaded {loaded}")]
    GameBuild { expected: u32, loaded: u32 },
    #[error("invalid Top Gear request: {0}")]
    Invalid(String),
    #[error("invalid SimulationCraft profileset result: {0}")]
    Profileset(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Enumerates all valid combinations and returns the exact count. If the caller's
/// safety cap is exceeded, a stable prefix is emitted while `valid_combinations`
/// still reports the complete search size.
pub fn generate_loadouts(rules: &RuleManifest, request: &SearchRequest) -> Result<SearchPreview> {
    validate_request(rules, request)?;
    let slots: Vec<_> = request.candidates.keys().copied().collect();
    let raw_combinations = slots.iter().try_fold(1_u64, |total, slot| {
        total
            .checked_mul(request.candidates[slot].len() as u64)
            .ok_or_else(|| Error::Invalid("combination count overflowed".into()))
    })?;
    if raw_combinations > MAX_RAW_COMBINATIONS {
        return Err(Error::Invalid(format!(
            "raw combination count {raw_combinations} exceeds the {MAX_RAW_COMBINATIONS} safety limit"
        )));
    }
    let pruned_candidates: BTreeMap<_, _> = request
        .candidates
        .iter()
        .map(|(slot, candidates)| (*slot, prune_dominated_variants(candidates)))
        .collect();
    let dominated_variants = request
        .candidates
        .values()
        .map(Vec::len)
        .sum::<usize>()
        .saturating_sub(pruned_candidates.values().map(Vec::len).sum::<usize>());
    let pruned_request = SearchRequest {
        candidates: pruned_candidates,
        ..request.clone()
    };

    let mut valid_combinations = 0_u64;
    let mut loadouts = Vec::new();
    let mut rejections = RejectionBreakdown {
        dominated_variants: dominated_variants as u64,
        ..RejectionBreakdown::default()
    };
    enumerate(
        rules,
        &pruned_request,
        &slots,
        0,
        &mut BTreeMap::new(),
        &mut valid_combinations,
        &mut loadouts,
        &mut rejections,
    )?;
    loadouts.sort_by(|left, right| left.key.cmp(&right.key));
    let was_truncated = valid_combinations > loadouts.len() as u64;
    Ok(SearchPreview {
        raw_combinations,
        valid_combinations,
        emitted_combinations: loadouts.len(),
        was_truncated,
        rejections,
        loadouts,
    })
}

#[allow(clippy::too_many_arguments)]
fn enumerate(
    rules: &RuleManifest,
    request: &SearchRequest,
    slots: &[GearSlot],
    index: usize,
    selected: &mut BTreeMap<GearSlot, ItemVariant>,
    valid_count: &mut u64,
    output: &mut Vec<Loadout>,
    rejections: &mut RejectionBreakdown,
) -> Result<()> {
    if index == slots.len() {
        match make_loadout(rules, request, selected)? {
            Ok(loadout) => {
                if is_canonical_symmetric_selection(&loadout.items) {
                    *valid_count = valid_count.checked_add(1).ok_or_else(|| {
                        Error::Invalid("valid combination count overflowed".into())
                    })?;
                    if output.len() < request.max_combinations {
                        output.push(loadout);
                    }
                } else {
                    rejections.symmetric_duplicate =
                        rejections.symmetric_duplicate.saturating_add(1);
                }
            }
            Err(reason) => rejections.record(reason),
        }
        return Ok(());
    }
    let slot = slots[index];
    for candidate in &request.candidates[&slot] {
        selected.insert(slot, candidate.clone());
        enumerate(
            rules,
            request,
            slots,
            index + 1,
            selected,
            valid_count,
            output,
            rejections,
        )?;
    }
    selected.remove(&slot);
    Ok(())
}

fn make_loadout(
    rules: &RuleManifest,
    request: &SearchRequest,
    selected: &BTreeMap<GearSlot, ItemVariant>,
) -> Result<std::result::Result<Loadout, RejectionReason>> {
    let mut unique = BTreeSet::new();
    let mut embellishments = 0_u8;
    let mut cost = CostVector::new();
    for (slot, item) in selected {
        if item.slot != *slot || item.source_item_id == 0 || item.key.is_empty() {
            return Err(Error::Invalid("candidate identity is inconsistent".into()));
        }
        validate_simc_options(&item.simc_options)?;
        validate_actions(item)?;
        if item.gem_ids.len() > usize::from(rules.max_sockets_per_item) {
            return Ok(Err(RejectionReason::SocketLimit));
        }
        if item.enchant_id.is_some() && !rules.allowed_enchant_slots.contains(slot) {
            return Ok(Err(RejectionReason::EnchantSlot));
        }
        if !item.unique_groups.iter().all(|group| unique.insert(group)) {
            return Ok(Err(RejectionReason::UniqueEquipped));
        }
        embellishments += u8::from(item.embellishment);
        add_cost(&mut cost, &item.cost)?;
    }
    if embellishments > rules.max_embellishments {
        return Ok(Err(RejectionReason::EmbellishmentLimit));
    }
    if !valid_weapons(selected) {
        return Ok(Err(RejectionReason::WeaponConstraint));
    }
    if !within_budget(&cost, &request.budget, &rules.currency_ids)? {
        return Ok(Err(RejectionReason::Budget));
    }
    let canonical = canonical_selection_key(selected);
    let key = short_hash(&canonical);
    Ok(Ok(Loadout {
        key,
        items: selected.clone(),
        cost,
        changed_slots: selected.values().filter(|item| item.changed).count(),
    }))
}

fn validate_actions(item: &ItemVariant) -> Result<()> {
    let ids: BTreeSet<_> = item
        .actions
        .iter()
        .map(|action| action.id.as_str())
        .collect();
    if ids.len() != item.actions.len() || ids.contains("") {
        return Err(Error::Invalid(
            "action ids must be unique and non-empty".into(),
        ));
    }
    let mut action_cost = CostVector::new();
    for action in &item.actions {
        if action.slot != item.slot || action.source_item_id != item.source_item_id {
            return Err(Error::Invalid(format!(
                "action {} targets a different item",
                action.id
            )));
        }
        validate_simc_options(&action.simc_options_patch)?;
        if action
            .simc_options_patch
            .iter()
            .any(|(key, value)| item.simc_options.get(key) != Some(value))
        {
            return Err(Error::Invalid(format!(
                "action {} does not match the final item options",
                action.id
            )));
        }
        if action
            .depends_on
            .iter()
            .any(|dependency| !ids.contains(dependency.as_str()))
        {
            return Err(Error::Invalid(format!(
                "action {} has an unknown dependency",
                action.id
            )));
        }
        match action.kind {
            ChangeKind::Upgrade => match (action.from_rank, action.to_rank) {
                (Some(from), Some(to)) if to == from.saturating_add(1) => {}
                _ => {
                    return Err(Error::Invalid(format!(
                        "upgrade action {} must advance exactly one rank",
                        action.id
                    )));
                }
            },
            _ if action.from_rank.is_some() || action.to_rank.is_some() => {
                return Err(Error::Invalid(format!(
                    "non-upgrade action {} cannot declare ranks",
                    action.id
                )));
            }
            _ => {}
        }
        add_cost(&mut action_cost, &action.cost)?;
    }
    if !item.actions.is_empty() && normalized_cost(&action_cost) != normalized_cost(&item.cost) {
        return Err(Error::Invalid(format!(
            "action costs do not equal final variant cost for {}",
            item.key
        )));
    }
    Ok(())
}

fn normalized_cost(cost: &CostVector) -> CostVector {
    cost.iter()
        .filter(|(_, amount)| **amount != 0)
        .map(|(currency, amount)| (currency.clone(), *amount))
        .collect()
}

fn prune_dominated_variants(candidates: &[ItemVariant]) -> Vec<ItemVariant> {
    candidates
        .iter()
        .enumerate()
        .filter(|(index, candidate)| {
            !candidates.iter().enumerate().any(|(other_index, other)| {
                other_index != *index
                    && same_final_tuple(other, candidate)
                    && cost_dominates(&other.cost, &candidate.cost)
                    && (normalized_cost(&other.cost) != normalized_cost(&candidate.cost)
                        || other.key < candidate.key)
            })
        })
        .map(|(_, candidate)| candidate.clone())
        .collect()
}

fn same_final_tuple(left: &ItemVariant, right: &ItemVariant) -> bool {
    left.source_item_id == right.source_item_id
        && left.slot == right.slot
        && left.rank == right.rank
        && left.gem_ids == right.gem_ids
        && left.enchant_id == right.enchant_id
        && left.simc_options == right.simc_options
        && left.unique_groups == right.unique_groups
        && left.weapon_kind == right.weapon_kind
        && left.embellishment == right.embellishment
}

fn cost_dominates(left: &CostVector, right: &CostVector) -> bool {
    left.keys().chain(right.keys()).all(|currency| {
        left.get(currency).copied().unwrap_or_default()
            <= right.get(currency).copied().unwrap_or_default()
    })
}

fn validate_simc_options(options: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in options {
        if !matches!(
            key.as_str(),
            "enchant_id"
                | "gem_id"
                | "bonus_id"
                | "gem_bonus_id"
                | "crafted_stats"
                | "crafting_quality"
                | "drop_level"
                | "content_tuning"
                | "redirected_base_stats"
                | "titan_disc_id"
                | "ilevel"
        ) || value.is_empty()
            || value.len() > 4096
            || !value.chars().all(|character| {
                character.is_alphanumeric()
                    || matches!(
                        character,
                        '_' | '-' | '.' | '/' | ':' | '+' | '%' | '@' | '='
                    )
            })
        {
            return Err(Error::Invalid(format!(
                "unsafe SimulationCraft item option: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_request(rules: &RuleManifest, request: &SearchRequest) -> Result<()> {
    if rules.schema_version != 1 {
        return Err(Error::Invalid("unsupported rule schema".into()));
    }
    if rules.revision != request.expected_rule_revision {
        return Err(Error::RuleRevision {
            expected: request.expected_rule_revision.clone(),
            loaded: rules.revision.clone(),
        });
    }
    if rules.game_build != request.game_build {
        return Err(Error::GameBuild {
            expected: request.game_build,
            loaded: rules.game_build,
        });
    }
    if request.candidates.is_empty()
        || request.candidates.values().any(Vec::is_empty)
        || !(1..=MAX_EMITTED_COMBINATIONS).contains(&request.max_combinations)
    {
        return Err(Error::Invalid(format!(
            "candidates must be non-empty and the emitted safety cap must be between 1 and {MAX_EMITTED_COMBINATIONS}"
        )));
    }
    for candidates in request.candidates.values() {
        if candidates.len() > MAX_CANDIDATES_PER_SLOT {
            return Err(Error::Invalid(format!(
                "candidate count per slot exceeds {MAX_CANDIDATES_PER_SLOT}"
            )));
        }
        let keys: BTreeSet<_> = candidates.iter().map(|candidate| &candidate.key).collect();
        if keys.len() != candidates.len() {
            return Err(Error::Invalid(
                "candidate keys must be unique within each slot".into(),
            ));
        }
    }
    for currency in request
        .budget
        .balances
        .keys()
        .chain(request.budget.reserves.keys())
    {
        if !rules.currency_ids.contains(currency) {
            return Err(Error::Invalid(format!("unknown currency: {currency}")));
        }
    }
    Ok(())
}

fn valid_weapons(items: &BTreeMap<GearSlot, ItemVariant>) -> bool {
    let main = items.get(&GearSlot::MainHand);
    let off = items.get(&GearSlot::OffHand);
    match (
        main.map(|item| item.weapon_kind),
        off.map(|item| item.weapon_kind),
    ) {
        (Some(WeaponKind::TwoHand), Some(kind)) => kind == WeaponKind::None,
        (Some(WeaponKind::TwoHand), None) => true,
        (Some(WeaponKind::OneHand), Some(kind)) => {
            matches!(
                kind,
                WeaponKind::OneHand | WeaponKind::OffHand | WeaponKind::None
            )
        }
        (Some(WeaponKind::OneHand), None) => true,
        (Some(WeaponKind::None), _) | (None, _) => true,
        _ => false,
    }
}

fn within_budget(
    cost: &CostVector,
    budget: &BudgetSnapshot,
    known: &BTreeSet<String>,
) -> Result<bool> {
    for (currency, amount) in cost {
        if !known.contains(currency) {
            return Err(Error::Invalid(format!("unknown currency: {currency}")));
        }
        let balance = budget.balances.get(currency).copied().unwrap_or_default();
        let reserve = budget.reserves.get(currency).copied().unwrap_or_default();
        if reserve > balance || *amount > balance - reserve {
            return Ok(false);
        }
    }
    Ok(true)
}

fn add_cost(target: &mut CostVector, source: &CostVector) -> Result<()> {
    for (currency, amount) in source {
        let entry = target.entry(currency.clone()).or_default();
        *entry = entry
            .checked_add(*amount)
            .ok_or_else(|| Error::Invalid("currency cost overflowed".into()))?;
    }
    Ok(())
}

fn canonical_selection_key(items: &BTreeMap<GearSlot, ItemVariant>) -> String {
    let mut parts: Vec<_> = items
        .iter()
        .map(|(slot, item)| (symmetry_group(*slot), item.key.as_str()))
        .collect();
    parts.sort_unstable();
    parts
        .into_iter()
        .map(|(slot, key)| format!("{slot}:{key}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn is_canonical_symmetric_selection(items: &BTreeMap<GearSlot, ItemVariant>) -> bool {
    [
        (GearSlot::Finger1, GearSlot::Finger2),
        (GearSlot::Trinket1, GearSlot::Trinket2),
    ]
    .into_iter()
    .all(
        |(left, right)| match (items.get(&left), items.get(&right)) {
            (Some(left), Some(right)) => left.key <= right.key,
            _ => true,
        },
    )
}

fn symmetry_group(slot: GearSlot) -> &'static str {
    match slot {
        GearSlot::Finger1 | GearSlot::Finger2 => "finger",
        GearSlot::Trinket1 | GearSlot::Trinket2 => "trinket",
        _ => slot.simc_token(),
    }
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Produces profileset overrides with short ASCII keys. The base input remains
/// byte-for-byte intact and each changed slot is appended as a profileset line.
pub fn build_profileset_input(base_input: &[u8], loadouts: &[Loadout]) -> Result<Vec<u8>> {
    build_profileset_stage_input(base_input, loadouts, None, None)
}

pub fn build_profileset_stage_input(
    base_input: &[u8],
    loadouts: &[Loadout],
    iterations: Option<u32>,
    profileset_work_threads: Option<u16>,
) -> Result<Vec<u8>> {
    if iterations == Some(0) || profileset_work_threads == Some(0) {
        return Err(Error::Invalid(
            "stage iterations and worker count must be positive".into(),
        ));
    }
    let mut output = String::from_utf8(base_input.to_vec())
        .map_err(|_| Error::Invalid("base input is not UTF-8".into()))?;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("\n# SimShredder Top Gear stage options\n");
    if let Some(iterations) = iterations {
        writeln!(output, "iterations={iterations}").expect("String writes cannot fail");
    }
    if let Some(workers) = profileset_work_threads {
        writeln!(output, "profileset_work_threads={workers}").expect("String writes cannot fail");
    }
    output.push_str("\n# SimShredder Top Gear profilesets\n");
    for loadout in loadouts {
        let mut changes = loadout.items.iter().filter(|(_, item)| item.changed);
        let Some((slot, item)) = changes.next() else {
            writeln!(output, "profileset.{}=default_actions=1", loadout.key)
                .expect("String writes cannot fail");
            continue;
        };
        write_profileset_item(&mut output, &loadout.key, false, *slot, item);
        for (slot, item) in changes {
            write_profileset_item(&mut output, &loadout.key, true, *slot, item);
        }
    }
    Ok(output.into_bytes())
}

fn write_profileset_item(
    output: &mut String,
    key: &str,
    append: bool,
    slot: GearSlot,
    item: &ItemVariant,
) {
    let operator = if append { "+=" } else { "=" };
    write!(
        output,
        "profileset.{key}{operator}{}=,id={}",
        slot.simc_token(),
        item.source_item_id
    )
    .expect("String writes cannot fail");
    for (option, value) in &item.simc_options {
        write!(output, ",{option}={value}").expect("String writes cannot fail");
    }
    output.push('\n');
}

pub fn parse_profileset_results(document: &Value) -> Result<Vec<ProfilesetResult>> {
    let results = document
        .pointer("/sim/profilesets/results")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Profileset("/sim/profilesets/results is missing".into()))?;
    results
        .iter()
        .map(|value| {
            let key = value
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Profileset("result name is missing".into()))?;
            if key.is_empty() || !key.chars().all(|character| character.is_ascii_hexdigit()) {
                return Err(Error::Profileset(format!("unsafe result key: {key}")));
            }
            Ok(ProfilesetResult {
                key: key.to_owned(),
                mean: json_number(value, &["mean"])?,
                mean_error: json_number(value, &["mean_error", "mean_std_dev"])?,
                iterations: value
                    .get("iterations")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Error::Profileset("iterations are missing".into()))?,
            })
        })
        .collect()
}

fn json_number(value: &Value, names: &[&str]) -> Result<f64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_f64))
        .filter(|number| number.is_finite() && *number >= 0.0)
        .ok_or_else(|| Error::Profileset(format!("numeric field {} is missing", names.join("/"))))
}

/// Ranks by mean, reports statistical equivalence to the baseline, and marks
/// results that are not dominated in both performance and every currency cost.
pub fn rank_results(
    baseline_mean: f64,
    baseline_error: f64,
    evaluations: Vec<EvaluatedLoadout>,
) -> Result<Vec<RankedLoadout>> {
    if !baseline_mean.is_finite()
        || baseline_mean < 0.0
        || !baseline_error.is_finite()
        || baseline_error < 0.0
    {
        return Err(Error::Invalid("baseline statistics are invalid".into()));
    }
    if evaluations.iter().any(|entry| {
        !entry.mean.is_finite()
            || entry.mean < 0.0
            || !entry.mean_error.is_finite()
            || entry.mean_error < 0.0
    }) {
        return Err(Error::Invalid("candidate statistics are invalid".into()));
    }
    let pareto: Vec<bool> = evaluations
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            !evaluations
                .iter()
                .enumerate()
                .any(|(other_index, other)| other_index != index && dominates(other, candidate))
        })
        .collect();
    let mut ranked: Vec<_> = evaluations
        .into_iter()
        .zip(pareto)
        .map(|(entry, pareto_optimal)| {
            let combined_error = baseline_error.hypot(entry.mean_error);
            let delta = entry.mean - baseline_mean;
            RankedLoadout {
                loadout: entry.loadout,
                mean: entry.mean,
                mean_error: entry.mean_error,
                delta,
                combined_error,
                equivalent_to_baseline: delta.abs() <= combined_error,
                pareto_optimal,
                rank: 0,
            }
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .mean
            .total_cmp(&left.mean)
            .then_with(|| left.loadout.key.cmp(&right.loadout.key))
    });
    for (index, entry) in ranked.iter_mut().enumerate() {
        entry.rank = index + 1;
    }
    Ok(ranked)
}

fn dominates(left: &EvaluatedLoadout, right: &EvaluatedLoadout) -> bool {
    if left.mean < right.mean {
        return false;
    }
    let currencies: BTreeSet<_> = left
        .loadout
        .cost
        .keys()
        .chain(right.loadout.cost.keys())
        .collect();
    let no_more_expensive = currencies.iter().all(|currency| {
        left.loadout
            .cost
            .get(*currency)
            .copied()
            .unwrap_or_default()
            <= right
                .loadout
                .cost
                .get(*currency)
                .copied()
                .unwrap_or_default()
    });
    let strictly_better = left.mean > right.mean
        || currencies.iter().any(|currency| {
            left.loadout
                .cost
                .get(*currency)
                .copied()
                .unwrap_or_default()
                < right
                    .loadout
                    .cost
                    .get(*currency)
                    .copied()
                    .unwrap_or_default()
        });
    no_more_expensive && strictly_better
}

/// Orders dependent actions by currently available marginal gain. `gains` is
/// intentionally supplied by the simulation layer so each step can be re-simmed.
pub fn plan_actions(
    actions: &[UpgradeAction],
    budget: &BudgetSnapshot,
    gains: &BTreeMap<String, f64>,
) -> Result<Vec<PlannedAction>> {
    let mut remaining = budget.balances.clone();
    for (currency, reserve) in &budget.reserves {
        let balance = remaining.get(currency).copied().unwrap_or_default();
        if *reserve > balance {
            return Err(Error::Invalid(format!(
                "reserve exceeds balance for {currency}"
            )));
        }
        remaining.insert(currency.clone(), balance - reserve);
    }
    let mut pending: BTreeMap<_, _> = actions
        .iter()
        .map(|action| (action.id.clone(), action))
        .collect();
    if pending.len() != actions.len() {
        return Err(Error::Invalid("action ids must be unique".into()));
    }
    let mut completed = BTreeSet::new();
    let mut planned = Vec::new();
    while !pending.is_empty() {
        let candidate = pending
            .values()
            .filter(|action| action.depends_on.iter().all(|id| completed.contains(id)))
            .filter(|action| affordable(&action.cost, &remaining))
            .max_by(|left, right| {
                gains
                    .get(&left.id)
                    .copied()
                    .unwrap_or_default()
                    .total_cmp(&gains.get(&right.id).copied().unwrap_or_default())
                    .then_with(|| right.id.cmp(&left.id))
            })
            .copied();
        let Some(action) = candidate else {
            return Err(Error::Invalid(
                "action dependencies are cyclic or unaffordable".into(),
            ));
        };
        for (currency, amount) in &action.cost {
            let balance = remaining.get(currency).copied().unwrap_or_default();
            remaining.insert(currency.clone(), balance - amount);
        }
        planned.push(PlannedAction {
            id: action.id.clone(),
            label: action.label.clone(),
            kind: action.kind,
            cost: action.cost.clone(),
            remaining: remaining.clone(),
            marginal_gain: gains.get(&action.id).copied().unwrap_or_default(),
            cumulative_gain: planned
                .last()
                .map(|step: &PlannedAction| step.cumulative_gain)
                .unwrap_or_default()
                + gains.get(&action.id).copied().unwrap_or_default(),
        });
        completed.insert(action.id.clone());
        pending.remove(&action.id);
    }
    Ok(planned)
}

/// Builds every dependency-valid intermediate state for a selected final loadout.
/// The empty state is included so the action-stage baseline is simulated under
/// exactly the same stage settings.
pub fn build_action_states(baseline: &Loadout, winner: &Loadout) -> Result<Vec<ActionState>> {
    let mut actions: Vec<_> = winner
        .items
        .values()
        .flat_map(|item| item.actions.iter().cloned())
        .collect();
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    if actions.len() > 12 {
        return Err(Error::Invalid(
            "action planning is limited to 12 actions per loadout".into(),
        ));
    }
    let ids: BTreeSet<_> = actions.iter().map(|action| action.id.clone()).collect();
    if ids.len() != actions.len() {
        return Err(Error::Invalid("winner action ids are not unique".into()));
    }
    let mut ordered = Vec::with_capacity(actions.len());
    let mut pending: BTreeMap<_, _> = actions
        .into_iter()
        .map(|action| (action.id.clone(), action))
        .collect();
    while !pending.is_empty() {
        let next = pending
            .values()
            .find(|action| {
                action.depends_on.iter().all(|dependency| {
                    ordered
                        .iter()
                        .any(|done: &UpgradeAction| &done.id == dependency)
                })
            })
            .map(|action| action.id.clone())
            .ok_or_else(|| Error::Invalid("winner action dependencies are cyclic".into()))?;
        ordered.push(
            pending
                .remove(&next)
                .expect("selected pending action exists"),
        );
    }
    let actions = ordered;
    let mut states = Vec::new();
    for mask in 0_u64..(1_u64 << actions.len()) {
        let applied: BTreeSet<_> = actions
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1_u64 << index) != 0)
            .map(|(_, action)| action.id.clone())
            .collect();
        if actions.iter().any(|action| {
            applied.contains(&action.id)
                && action
                    .depends_on
                    .iter()
                    .any(|dependency| !applied.contains(dependency))
        }) {
            continue;
        }
        let mut items = baseline.items.clone();
        let mut cost = CostVector::new();
        for action in actions.iter().filter(|action| applied.contains(&action.id)) {
            let item = items.get_mut(&action.slot).ok_or_else(|| {
                Error::Invalid(format!("baseline item is missing for {}", action.id))
            })?;
            if item.source_item_id != action.source_item_id {
                item.simc_options.clear();
            }
            item.source_item_id = action.source_item_id;
            item.changed = true;
            item.key = format!("action-{}", action.id);
            for (option, value) in &action.simc_options_patch {
                item.simc_options.insert(option.clone(), value.clone());
            }
            if let Some(rank) = action.to_rank {
                item.rank = rank;
            }
            add_cost(&mut cost, &action.cost)?;
        }
        let state_identity = format!(
            "{}|{}",
            winner.key,
            applied.iter().cloned().collect::<Vec<_>>().join("|")
        );
        states.push(ActionState {
            loadout: Loadout {
                key: short_hash(&state_identity),
                changed_slots: items.values().filter(|item| item.changed).count(),
                items,
                cost,
            },
            applied_action_ids: applied,
        });
    }
    states.sort_by(|left, right| left.loadout.key.cmp(&right.loadout.key));
    Ok(states)
}

/// Derives a dependency-aware order from actual metrics for every intermediate
/// action state, recalculating marginal gain after each chosen step.
pub fn derive_action_plan(
    actions: &[UpgradeAction],
    states: &[ActionState],
    evaluations: &[ProfilesetResult],
    budget: &BudgetSnapshot,
) -> Result<Vec<PlannedAction>> {
    let metrics: BTreeMap<_, _> = evaluations
        .iter()
        .map(|result| (result.key.as_str(), result.mean))
        .collect();
    if metrics.len() != states.len() {
        return Err(Error::Invalid(
            "action-state result count does not match the preview".into(),
        ));
    }
    let mut remaining = budget.balances.clone();
    for (currency, reserve) in &budget.reserves {
        let balance = remaining.get(currency).copied().unwrap_or_default();
        if *reserve > balance {
            return Err(Error::Invalid(format!(
                "reserve exceeds balance for {currency}"
            )));
        }
        remaining.insert(currency.clone(), balance - reserve);
    }
    let mut applied = BTreeSet::new();
    let mut output = Vec::new();
    let baseline_mean = state_mean(states, &metrics, &applied)?;
    let mut current_mean = baseline_mean;
    while applied.len() < actions.len() {
        let mut candidates = Vec::new();
        for action in actions.iter().filter(|action| {
            !applied.contains(&action.id)
                && action
                    .depends_on
                    .iter()
                    .all(|dependency| applied.contains(dependency))
                && affordable(&action.cost, &remaining)
        }) {
            let mut next = applied.clone();
            next.insert(action.id.clone());
            let mean = state_mean(states, &metrics, &next)?;
            candidates.push((action, mean - current_mean, mean));
        }
        let Some((action, marginal_gain, next_mean)) =
            candidates
                .into_iter()
                .max_by(|(left, left_gain, _), (right, right_gain, _)| {
                    left_gain
                        .total_cmp(right_gain)
                        .then_with(|| right.id.cmp(&left.id))
                })
        else {
            return Err(Error::Invalid(
                "action dependencies are cyclic or unaffordable".into(),
            ));
        };
        for (currency, amount) in &action.cost {
            let balance = remaining.get(currency).copied().unwrap_or_default();
            remaining.insert(currency.clone(), balance - amount);
        }
        applied.insert(action.id.clone());
        current_mean = next_mean;
        output.push(PlannedAction {
            id: action.id.clone(),
            label: action.label.clone(),
            kind: action.kind,
            cost: action.cost.clone(),
            remaining: remaining.clone(),
            marginal_gain,
            cumulative_gain: current_mean - baseline_mean,
        });
    }
    Ok(output)
}

fn state_mean(
    states: &[ActionState],
    metrics: &BTreeMap<&str, f64>,
    applied: &BTreeSet<String>,
) -> Result<f64> {
    let state = states
        .iter()
        .find(|state| &state.applied_action_ids == applied)
        .ok_or_else(|| Error::Invalid("required action state was not simulated".into()))?;
    metrics
        .get(state.loadout.key.as_str())
        .copied()
        .ok_or_else(|| Error::Invalid("action-state metric is missing".into()))
}

fn affordable(cost: &CostVector, remaining: &CostVector) -> bool {
    cost.iter()
        .all(|(currency, amount)| *amount <= remaining.get(currency).copied().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn rules() -> RuleManifest {
        RuleManifest {
            schema_version: 1,
            revision: "test-v1".into(),
            game_build: 69465,
            game_version: "12.1.0".into(),
            source: "test fixture".into(),
            currency_ids: BTreeSet::from(["crest".into(), "valor".into()]),
            max_embellishments: 2,
            max_sockets_per_item: 3,
            allowed_enchant_slots: BTreeSet::from([
                GearSlot::Finger1,
                GearSlot::Finger2,
                GearSlot::MainHand,
            ]),
        }
    }

    #[test]
    fn bundled_rule_manifest_is_parseable_and_versioned() {
        let manifest: RuleManifest = serde_json::from_str(include_str!(
            "../../../../resources/rules/12.1.0-69465-v1.json"
        ))
        .unwrap();
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.revision, "12.1.0-69465-v1");
        assert_eq!(manifest.game_build, 69465);
    }

    fn variant(key: &str, slot: GearSlot, cost: u32) -> ItemVariant {
        ItemVariant {
            key: key.into(),
            source_item_id: key.bytes().map(u32::from).sum::<u32>() + 1,
            slot,
            rank: 1,
            gem_ids: Vec::new(),
            enchant_id: None,
            simc_options: BTreeMap::new(),
            cost: BTreeMap::from([("crest".into(), cost)]),
            actions: Vec::new(),
            unique_groups: BTreeSet::new(),
            weapon_kind: WeaponKind::None,
            embellishment: false,
            changed: key != "worn",
        }
    }

    fn request(candidates: BTreeMap<GearSlot, Vec<ItemVariant>>) -> SearchRequest {
        SearchRequest {
            expected_rule_revision: "test-v1".into(),
            game_build: 69465,
            candidates,
            budget: BudgetSnapshot {
                balances: BTreeMap::from([("crest".into(), 10), ("valor".into(), 20)]),
                reserves: BTreeMap::from([("crest".into(), 2)]),
                confirmed_at_unix_seconds: 1,
            },
            max_combinations: 100,
        }
    }

    #[test]
    fn removes_symmetric_ring_permutations_and_reports_exact_counts() {
        let a = variant("a", GearSlot::Finger1, 0);
        let b = variant("b", GearSlot::Finger1, 0);
        let mut candidates = BTreeMap::new();
        candidates.insert(GearSlot::Finger1, vec![a.clone(), b.clone()]);
        candidates.insert(
            GearSlot::Finger2,
            vec![
                ItemVariant {
                    slot: GearSlot::Finger2,
                    ..a
                },
                ItemVariant {
                    slot: GearSlot::Finger2,
                    ..b
                },
            ],
        );
        let preview = generate_loadouts(&rules(), &request(candidates)).unwrap();
        assert_eq!(preview.raw_combinations, 4);
        assert_eq!(preview.valid_combinations, 3);
        assert_eq!(preview.emitted_combinations, 3);
        assert_eq!(preview.rejections.symmetric_duplicate, 1);
    }

    #[test]
    fn large_preview_keeps_exact_counts_with_bounded_emitted_loadouts() {
        let candidates = BTreeMap::from([
            (
                GearSlot::Head,
                (0..10)
                    .map(|index| variant(&format!("head-{index}"), GearSlot::Head, 0))
                    .collect(),
            ),
            (
                GearSlot::Chest,
                (0..10)
                    .map(|index| variant(&format!("chest-{index}"), GearSlot::Chest, 0))
                    .collect(),
            ),
            (
                GearSlot::Hands,
                (0..10)
                    .map(|index| variant(&format!("hands-{index}"), GearSlot::Hands, 0))
                    .collect(),
            ),
            (
                GearSlot::Legs,
                (0..10)
                    .map(|index| variant(&format!("legs-{index}"), GearSlot::Legs, 0))
                    .collect(),
            ),
        ]);
        let mut request = request(candidates);
        request.max_combinations = 32;
        let preview = generate_loadouts(&rules(), &request).unwrap();
        assert_eq!(preview.raw_combinations, 10_000);
        assert_eq!(preview.valid_combinations, 10_000);
        assert_eq!(preview.emitted_combinations, 32);
        assert!(preview.was_truncated);
    }

    #[test]
    fn raw_combination_safety_limit_rejects_unbounded_work() {
        let candidates = [GearSlot::Head, GearSlot::Chest, GearSlot::Legs]
            .into_iter()
            .map(|slot| {
                let variants = (0..127)
                    .map(|index| variant(&format!("{}-{index}", slot.simc_token()), slot, 0))
                    .collect();
                (slot, variants)
            })
            .collect();
        let error = generate_loadouts(&rules(), &request(candidates)).unwrap_err();
        assert!(error.to_string().contains("safety limit"));
    }

    #[test]
    fn exhaustive_oracle_matches_budget_filtered_product() {
        let candidates = BTreeMap::from([
            (
                GearSlot::Head,
                vec![
                    variant("worn", GearSlot::Head, 0),
                    variant("head", GearSlot::Head, 4),
                ],
            ),
            (
                GearSlot::Chest,
                vec![
                    variant("worn", GearSlot::Chest, 0),
                    variant("chest", GearSlot::Chest, 6),
                    variant("too-much", GearSlot::Chest, 9),
                ],
            ),
        ]);
        let preview = generate_loadouts(&rules(), &request(candidates.clone())).unwrap();
        let oracle = candidates[&GearSlot::Head]
            .iter()
            .flat_map(|head| {
                candidates[&GearSlot::Chest]
                    .iter()
                    .map(move |chest| head.cost["crest"] + chest.cost["crest"])
            })
            .filter(|cost| *cost <= 8)
            .count();
        assert_eq!(preview.valid_combinations as usize, oracle);
        assert_eq!(preview.rejections.budget, 3);
    }

    #[test]
    fn rejects_rule_mismatch_and_invalid_enchant() {
        let mut bad = request(BTreeMap::from([(
            GearSlot::Head,
            vec![variant("worn", GearSlot::Head, 0)],
        )]));
        bad.expected_rule_revision = "old".into();
        assert!(matches!(
            generate_loadouts(&rules(), &bad),
            Err(Error::RuleRevision { .. })
        ));

        let mut enchanted = variant("enchanted", GearSlot::Head, 0);
        enchanted.enchant_id = Some(42);
        let preview = generate_loadouts(
            &rules(),
            &request(BTreeMap::from([(GearSlot::Head, vec![enchanted])])),
        )
        .unwrap();
        assert_eq!(preview.valid_combinations, 0);
        assert_eq!(preview.rejections.enchant_slot, 1);
    }

    #[test]
    fn removes_same_tuple_variant_with_dominated_cost_and_rejects_injection() {
        let cheap = variant("cheap", GearSlot::Head, 1);
        let costly = ItemVariant {
            key: "costly".into(),
            cost: BTreeMap::from([("crest".into(), 2)]),
            ..cheap.clone()
        };
        let preview = generate_loadouts(
            &rules(),
            &request(BTreeMap::from([(GearSlot::Head, vec![cheap, costly])])),
        )
        .unwrap();
        assert_eq!(preview.raw_combinations, 2);
        assert_eq!(preview.valid_combinations, 1);
        assert_eq!(preview.loadouts[0].cost["crest"], 1);
        assert_eq!(preview.rejections.dominated_variants, 1);

        let mut injected = variant("injected", GearSlot::Head, 0);
        injected
            .simc_options
            .insert("ilevel".into(), "700\njson2=/tmp/leak".into());
        assert!(
            generate_loadouts(
                &rules(),
                &request(BTreeMap::from([(GearSlot::Head, vec![injected])]))
            )
            .is_err()
        );
    }

    #[test]
    fn enforces_unique_embellishment_and_weapon_constraints() {
        let mut first = variant("first", GearSlot::Finger1, 0);
        first.unique_groups.insert("unique-ring".into());
        let mut second = variant("second", GearSlot::Finger2, 0);
        second.unique_groups.insert("unique-ring".into());
        let preview = generate_loadouts(
            &rules(),
            &request(BTreeMap::from([
                (GearSlot::Finger1, vec![first]),
                (GearSlot::Finger2, vec![second]),
            ])),
        )
        .unwrap();
        assert_eq!(preview.valid_combinations, 0);
        assert_eq!(preview.rejections.unique_equipped, 1);

        let mut main = variant("two-hand", GearSlot::MainHand, 0);
        main.weapon_kind = WeaponKind::TwoHand;
        let mut off = variant("shield", GearSlot::OffHand, 0);
        off.weapon_kind = WeaponKind::OffHand;
        let preview = generate_loadouts(
            &rules(),
            &request(BTreeMap::from([
                (GearSlot::MainHand, vec![main]),
                (GearSlot::OffHand, vec![off]),
            ])),
        )
        .unwrap();
        assert_eq!(preview.valid_combinations, 0);
        assert_eq!(preview.rejections.weapon_constraint, 1);
    }

    #[test]
    fn profileset_input_uses_changed_items_only() {
        let changed = variant("head", GearSlot::Head, 4);
        let loadout = Loadout {
            key: "0123abcd".into(),
            items: BTreeMap::from([(GearSlot::Head, changed)]),
            cost: BTreeMap::new(),
            changed_slots: 1,
        };
        let text =
            String::from_utf8(build_profileset_input(b"warrior=Tester\n", &[loadout]).unwrap())
                .unwrap();
        assert!(text.contains("profileset.0123abcd=head=,id="));
    }

    #[test]
    fn parses_profileset_results() {
        let document = serde_json::json!({"sim":{"profilesets":{"results":[{
            "name":"0123abcd","mean":123.0,"mean_error":2.5,"iterations":100
        }]}}});
        assert_eq!(parse_profileset_results(&document).unwrap()[0].mean, 123.0);
    }

    #[test]
    fn ranks_equivalence_and_multi_currency_pareto() {
        let base = Loadout {
            key: "cheap".into(),
            items: BTreeMap::new(),
            cost: BTreeMap::from([("crest".into(), 1)]),
            changed_slots: 1,
        };
        let costly = Loadout {
            key: "costly".into(),
            cost: BTreeMap::from([("crest".into(), 2)]),
            ..base.clone()
        };
        let ranked = rank_results(
            100.0,
            1.0,
            vec![
                EvaluatedLoadout {
                    loadout: base,
                    mean: 101.0,
                    mean_error: 1.0,
                },
                EvaluatedLoadout {
                    loadout: costly,
                    mean: 101.0,
                    mean_error: 1.0,
                },
            ],
        )
        .unwrap();
        assert!(
            ranked
                .iter()
                .find(|entry| entry.loadout.key == "cheap")
                .unwrap()
                .pareto_optimal
        );
        assert!(
            !ranked
                .iter()
                .find(|entry| entry.loadout.key == "costly")
                .unwrap()
                .pareto_optimal
        );
        assert!(ranked[0].equivalent_to_baseline);
    }

    #[test]
    fn orders_actions_by_gain_while_respecting_dependencies_and_reserve() {
        let action = |id: &str, gain: f64, dependency: Option<&str>| {
            (
                UpgradeAction {
                    id: id.into(),
                    label: id.into(),
                    kind: ChangeKind::Upgrade,
                    cost: BTreeMap::from([("crest".into(), 2)]),
                    depends_on: dependency.into_iter().map(str::to_owned).collect(),
                    from_rank: Some(0),
                    to_rank: Some(1),
                    slot: GearSlot::Head,
                    source_item_id: 1,
                    simc_options_patch: BTreeMap::new(),
                },
                gain,
            )
        };
        let (a, a_gain) = action("a", 5.0, None);
        let (b, b_gain) = action("b", 20.0, Some("a"));
        let (c, c_gain) = action("c", 10.0, None);
        let gains = BTreeMap::from([
            ("a".into(), a_gain),
            ("b".into(), b_gain),
            ("c".into(), c_gain),
        ]);
        let plan = plan_actions(
            &[a, b, c],
            &BudgetSnapshot {
                balances: BTreeMap::from([("crest".into(), 8)]),
                reserves: BTreeMap::from([("crest".into(), 2)]),
                confirmed_at_unix_seconds: 1,
            },
            &gains,
        )
        .unwrap();
        assert_eq!(
            plan.iter().map(|step| step.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "a", "b"]
        );
        assert_eq!(plan.last().unwrap().remaining["crest"], 0);
    }

    #[test]
    fn action_states_drive_recalculated_marginal_order() {
        let baseline_item = variant("worn", GearSlot::Head, 0);
        let baseline = Loadout {
            key: "baseline".into(),
            items: BTreeMap::from([(GearSlot::Head, baseline_item.clone())]),
            cost: BTreeMap::new(),
            changed_slots: 0,
        };
        let action = |id: &str, option: &str, value: &str| UpgradeAction {
            id: id.into(),
            label: id.into(),
            kind: ChangeKind::Gem,
            cost: BTreeMap::from([("crest".into(), 1)]),
            depends_on: Vec::new(),
            from_rank: None,
            to_rank: None,
            slot: GearSlot::Head,
            source_item_id: baseline_item.source_item_id,
            simc_options_patch: BTreeMap::from([(option.into(), value.into())]),
        };
        let actions = vec![action("a", "gem_id", "1"), action("b", "enchant_id", "2")];
        let mut winner_item = baseline_item;
        winner_item.changed = true;
        winner_item.actions = actions.clone();
        winner_item.cost = BTreeMap::from([("crest".into(), 2)]);
        let winner = Loadout {
            key: "winner".into(),
            items: BTreeMap::from([(GearSlot::Head, winner_item)]),
            cost: BTreeMap::from([("crest".into(), 2)]),
            changed_slots: 1,
        };
        let states = build_action_states(&baseline, &winner).unwrap();
        assert_eq!(states.len(), 4);
        let evaluations: Vec<_> = states
            .iter()
            .map(|state| {
                let mean = match (
                    state.applied_action_ids.contains("a"),
                    state.applied_action_ids.contains("b"),
                ) {
                    (false, false) => 100.0,
                    (true, false) => 110.0,
                    (false, true) => 108.0,
                    (true, true) => 125.0,
                };
                ProfilesetResult {
                    key: state.loadout.key.clone(),
                    mean,
                    mean_error: 1.0,
                    iterations: 100,
                }
            })
            .collect();
        let plan = derive_action_plan(
            &actions,
            &states,
            &evaluations,
            &BudgetSnapshot {
                balances: BTreeMap::from([("crest".into(), 3)]),
                reserves: BTreeMap::from([("crest".into(), 1)]),
                confirmed_at_unix_seconds: 1,
            },
        )
        .unwrap();
        assert_eq!(
            plan.iter().map(|step| step.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(plan[1].marginal_gain, 15.0);
        assert_eq!(plan[1].cumulative_gain, 25.0);
        assert_eq!(plan[1].remaining["crest"], 0);
    }
}

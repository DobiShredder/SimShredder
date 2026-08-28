use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use simshredder_domain::{
    NormalizedQuickResult, ResultAction, ResultAplAction, ResultBuff, ResultOptions, ResultPlayer,
    ResultResource, ResultRuntimeIdentity, StatisticalMetric,
};

use crate::{Error, Result, SimcIdentity};

pub const NORMALIZED_SCHEMA_VERSION: u32 = 2;
const SUPPORTED_REPORT_VERSION: &str = "2.0.0";
const MAX_ACTIONS: usize = 256;
const MAX_BUFFS: usize = 256;
const MAX_APL_ACTIONS: usize = 100;

pub fn normalize_quick_result(
    bytes: &[u8],
    expected_identity: &SimcIdentity,
    expected_revision: &str,
) -> Result<NormalizedQuickResult> {
    let document: Value = serde_json::from_slice(bytes)?;
    if string(&document, "/report_version")? != SUPPORTED_REPORT_VERSION {
        return contract("unsupported SimC JSON report version");
    }
    let simc_version = string(&document, "/version")?;
    let revision = string(&document, "/git_revision")?;
    if simc_version != expected_identity.simc_version || revision != expected_revision {
        return contract("JSON report runtime identity does not match the validated executable");
    }
    if string(&document, "/sim/options/dbc/version_used")? != "Live" {
        return contract("JSON report did not use Retail Live data");
    }
    let game_version = string(&document, "/sim/options/dbc/Live/wow_version")?;
    if expected_identity.channel != "live" || game_version != expected_identity.game_version {
        return contract("JSON report game identity does not match the Live executable");
    }

    let players = document
        .pointer("/sim/players")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Contract("JSON report players are missing".into()))?;
    if players.len() != 1 {
        return contract("Quick Sim requires exactly one player");
    }
    let player = &players[0];
    let role = string(player, "/role")?;
    let metric_name = match role.as_str() {
        "attack" | "tank" | "melee" | "spell" | "hybrid" | "dps" => "dps",
        "heal" => "hps",
        // SimC reports `auto` for otherwise valid damage actors when an imported
        // profile used one of its historical DPS role aliases. The collected
        // metric is the authoritative execution result in that case.
        "auto" if player.pointer("/collected_data/dps").is_some() => "dps",
        "auto" if player.pointer("/collected_data/hps").is_some() => "hps",
        _ => return contract(format!("unsupported result role: {role}")),
    };
    let metric = player
        .pointer(&format!("/collected_data/{metric_name}"))
        .ok_or_else(|| Error::Contract(format!("{metric_name} metric is missing")))?;
    let primary_metric = StatisticalMetric {
        name: metric_name.into(),
        mean: number(metric, "/mean")?,
        mean_error: number(metric, "/mean_std_dev")?,
        standard_deviation: number(metric, "/std_dev")?,
        minimum: number(metric, "/min")?,
        maximum: number(metric, "/max")?,
        median: number(metric, "/median")?,
    };
    if primary_metric.mean < 0.0
        || primary_metric.mean_error < 0.0
        || primary_metric.standard_deviation < 0.0
    {
        return contract("primary metric contains negative statistics");
    }

    let game_build = integer(&document, "/sim/options/dbc/Live/build_level")?;
    let actions = normalize_actions(player)?;
    let buffs = normalize_buffs(player)?;
    let resources = normalize_resources(player)?;
    let apl_sequence = normalize_apl_sequence(player)?;
    Ok(NormalizedQuickResult {
        schema_version: NORMALIZED_SCHEMA_VERSION,
        report_version: SUPPORTED_REPORT_VERSION.into(),
        runtime: ResultRuntimeIdentity {
            simc_version,
            git_revision: revision,
            game_version,
            game_build: u32::try_from(game_build)
                .map_err(|_| Error::Contract("game build is too large".into()))?,
            channel: "live".into(),
        },
        player: ResultPlayer {
            name: string(player, "/name")?,
            race: string(player, "/race")?,
            role,
            specialization: string(player, "/specialization")?,
        },
        options: ResultOptions {
            iterations: integer(&document, "/sim/options/iterations")?,
            threads: integer(&document, "/sim/options/threads")?,
            seed: integer(&document, "/sim/options/seed")?,
            max_time_seconds: number(&document, "/sim/options/max_time")?,
            desired_targets: integer(&document, "/sim/options/desired_targets")?,
            fight_style: string(&document, "/sim/options/fight_style")?,
        },
        primary_metric,
        actions,
        buffs,
        resources,
        apl_sequence,
    })
}

fn normalize_actions(player: &Value) -> Result<Vec<ResultAction>> {
    let Some(entries) = optional_array(player, "/stats")? else {
        return Ok(Vec::new());
    };
    let mut actions = Vec::new();
    for entry in entries {
        let kind = required_bounded_string(entry, "/type")?;
        if !matches!(kind.as_str(), "damage" | "heal") {
            continue;
        }
        let amount = number(entry, "/compound_amount")?;
        let per_second = optional_number(entry, "/portion_aps/mean")?;
        let share = optional_number(entry, "/portion_amount")?;
        let (per_second, share) = match (per_second, share) {
            (Some(per_second), Some(share)) => (per_second, share),
            (None, None) => continue,
            _ => return contract("action contribution fields are incomplete"),
        };
        let executes = number(entry, "/num_executes/mean")?;
        if amount < 0.0 || per_second < 0.0 || executes < 0.0 || !(0.0..=1.0).contains(&share) {
            return contract("action statistics are outside their supported range");
        }
        if amount == 0.0 {
            continue;
        }
        let internal_name = required_bounded_string(entry, "/name")?;
        actions.push(ResultAction {
            id: optional_positive_u32(entry.pointer("/id"), "/stats/id")?,
            name: display_name(entry, "/spell_name", &internal_name)?,
            internal_name,
            school: optional_bounded_string(entry, "/school")?.unwrap_or_else(|| "unknown".into()),
            executes,
            amount_per_fight: amount,
            metric_per_second: per_second,
            share,
        });
    }
    actions.sort_by(|left, right| {
        right
            .share
            .total_cmp(&left.share)
            .then_with(|| left.name.cmp(&right.name))
    });
    actions.truncate(MAX_ACTIONS);
    Ok(actions)
}

fn normalize_buffs(player: &Value) -> Result<Vec<ResultBuff>> {
    let Some(entries) = optional_array(player, "/buffs")? else {
        return Ok(Vec::new());
    };
    let mut buffs = Vec::new();
    for entry in entries {
        let internal_name = required_bounded_string(entry, "/name")?;
        let uptime = number(entry, "/uptime")?;
        let benefit = optional_number(entry, "/benefit")?;
        let starts = optional_number(entry, "/start_count")?.unwrap_or(0.0);
        if !(0.0..=100.0).contains(&uptime)
            || benefit.is_some_and(|value| !(0.0..=100.0).contains(&value))
            || starts < 0.0
        {
            return contract("buff statistics are outside their supported range");
        }
        buffs.push(ResultBuff {
            id: optional_positive_u32(entry.pointer("/spell"), "/buffs/spell")?,
            name: display_name(entry, "/spell_name", &internal_name)?,
            internal_name,
            uptime_percent: uptime,
            benefit_percent: benefit,
            starts,
        });
    }
    buffs.sort_by(|left, right| {
        right
            .uptime_percent
            .total_cmp(&left.uptime_percent)
            .then_with(|| left.name.cmp(&right.name))
    });
    buffs.truncate(MAX_BUFFS);
    Ok(buffs)
}

fn normalize_resources(player: &Value) -> Result<Vec<ResultResource>> {
    let collected = player
        .pointer("/collected_data")
        .ok_or_else(|| Error::Contract("collected data is missing".into()))?;
    let lost = optional_object(collected, "/resource_lost")?;
    let overflow = optional_object(collected, "/resource_overflowed")?;
    let end = optional_object(collected, "/combat_end_resource")?;
    let keys = lost
        .into_iter()
        .flat_map(|object| object.keys())
        .chain(overflow.into_iter().flat_map(|object| object.keys()))
        .chain(end.into_iter().flat_map(|object| object.keys()))
        .cloned()
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .map(|name| {
            let name = bounded_string(&name)
                .ok_or_else(|| Error::Contract("resource name is invalid".into()))?;
            Ok(ResultResource {
                spent_per_fight: mean_for_key(collected, "/resource_lost", &name)?,
                overflow_per_fight: mean_for_key(collected, "/resource_overflowed", &name)?,
                remaining_per_fight: mean_for_key(collected, "/combat_end_resource", &name)?,
                name,
            })
        })
        .collect()
}

fn normalize_apl_sequence(player: &Value) -> Result<Vec<ResultAplAction>> {
    let Some(entries) = optional_array(player, "/collected_data/action_sequence")? else {
        return Ok(Vec::new());
    };
    let mut actions = Vec::new();
    for entry in entries {
        let internal_name = match optional_bounded_string(entry, "/name")? {
            Some(name) => name,
            None if optional_number(entry, "/wait")?.is_some_and(|wait| wait >= 0.0) => continue,
            None => return contract("action sequence entry has neither an action nor a wait"),
        };
        let spell_name = entry
            .pointer("/spell_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let time_seconds = number(entry, "/time")?;
        if time_seconds < 0.0 {
            return contract("action sequence time is outside its supported range");
        }
        actions.push(ResultAplAction {
            time_seconds,
            id: optional_positive_u32(entry.pointer("/id"), "/action_sequence/id")?,
            name: if spell_name.is_empty() {
                display_internal_name(&internal_name)
            } else {
                required_bounded_string(entry, "/spell_name")?
            },
            internal_name,
            target: required_bounded_string(entry, "/target")?,
            resources: finite_number_map(entry, "/resources")?,
            resource_max: finite_number_map(entry, "/resources_max")?,
        });
        if actions.len() == MAX_APL_ACTIONS {
            break;
        }
    }
    Ok(actions)
}

fn optional_array<'a>(value: &'a Value, pointer: &str) -> Result<Option<&'a Vec<Value>>> {
    match value.pointer(pointer) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(entries)) => Ok(Some(entries)),
        Some(_) => contract(format!("JSON array is invalid at {pointer}")),
    }
}

fn optional_object<'a>(
    value: &'a Value,
    pointer: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>> {
    match value.pointer(pointer) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => Ok(Some(object)),
        Some(_) => contract(format!("JSON object is invalid at {pointer}")),
    }
}

fn mean_for_key(value: &Value, pointer: &str, key: &str) -> Result<f64> {
    let Some(object) = optional_object(value, pointer)? else {
        return Ok(0.0);
    };
    let Some(entry) = object.get(key) else {
        return Ok(0.0);
    };
    let mean = entry
        .pointer("/mean")
        .and_then(finite_number)
        .ok_or_else(|| Error::Contract(format!("resource mean is invalid at {pointer}/{key}")))?;
    if mean < 0.0 {
        return contract(format!("resource mean is negative at {pointer}/{key}"));
    }
    Ok(mean)
}

fn finite_number_map(value: &Value, pointer: &str) -> Result<BTreeMap<String, f64>> {
    let Some(object) = optional_object(value, pointer)? else {
        return Ok(BTreeMap::new());
    };
    object
        .iter()
        .map(|(key, value)| {
            let key = bounded_string(key)
                .ok_or_else(|| Error::Contract(format!("map key is invalid at {pointer}")))?;
            let value = finite_number(value)
                .filter(|value| *value >= 0.0)
                .ok_or_else(|| {
                    Error::Contract(format!("map value is invalid at {pointer}/{key}"))
                })?;
            Ok((key, value))
        })
        .collect()
}

fn optional_positive_u32(value: Option<&Value>, pointer: &str) -> Result<Option<u32>> {
    let Some(value) = value else { return Ok(None) };
    let value = value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::Contract(format!("non-negative ID is invalid at {pointer}")))?;
    Ok((value > 0).then_some(value))
}

fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn bounded_string(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|character| matches!(character, '\0' | '\n' | '\r')))
    .then(|| value.to_owned())
}

fn optional_bounded_string(value: &Value, pointer: &str) -> Result<Option<String>> {
    match value.pointer(pointer) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) => bounded_string(text)
            .map(Some)
            .ok_or_else(|| Error::Contract(format!("JSON string is invalid at {pointer}"))),
        Some(_) => contract(format!("JSON string is invalid at {pointer}")),
    }
}

fn optional_number(value: &Value, pointer: &str) -> Result<Option<f64>> {
    match value.pointer(pointer) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => finite_number(value)
            .map(Some)
            .ok_or_else(|| Error::Contract(format!("JSON number is invalid at {pointer}"))),
    }
}

fn display_name(value: &Value, pointer: &str, internal_name: &str) -> Result<String> {
    Ok(optional_bounded_string(value, pointer)?
        .unwrap_or_else(|| display_internal_name(internal_name)))
}

fn required_bounded_string(value: &Value, pointer: &str) -> Result<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .and_then(bounded_string)
        .ok_or_else(|| Error::Contract(format!("JSON string is invalid at {pointer}")))
}

fn display_internal_name(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().chain(characters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn string(value: &Value, pointer: &str) -> Result<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::Contract(format!("JSON string is missing at {pointer}")))
}

fn integer(value: &Value, pointer: &str) -> Result<u64> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Contract(format!("JSON integer is missing at {pointer}")))
}

fn number(value: &Value, pointer: &str) -> Result<f64> {
    value
        .pointer(pointer)
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
        .ok_or_else(|| Error::Contract(format!("JSON number is missing at {pointer}")))
}

fn contract<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Contract(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> SimcIdentity {
        SimcIdentity {
            simc_version: "1210-01".into(),
            game_version: "12.1.0.69465".into(),
            channel: "live".into(),
            hotfix: Some("2026-08-24/69465".into()),
        }
    }

    #[test]
    fn rejects_ptr_execution_and_unknown_report_versions() {
        let mut document: Value = serde_json::from_str(include_str!(
            "../../../../test-data/fixtures/reports/quick-1210-01-3487fce.min.json"
        ))
        .unwrap();
        document["sim"]["options"]["dbc"]["version_used"] = Value::String("PTR".into());
        assert!(
            normalize_quick_result(
                &serde_json::to_vec(&document).unwrap(),
                &identity(),
                "3487fce"
            )
            .is_err()
        );
        document["sim"]["options"]["dbc"]["version_used"] = Value::String("Live".into());
        document["report_version"] = Value::String("9.0.0".into());
        assert!(
            normalize_quick_result(
                &serde_json::to_vec(&document).unwrap(),
                &identity(),
                "3487fce"
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_malformed_detailed_result_fields() {
        let fixture =
            include_str!("../../../../test-data/fixtures/reports/quick-1210-01-3487fce.min.json");
        let mutations = [
            (
                "/sim/players/0/stats/0/portion_amount",
                serde_json::json!(1.1),
            ),
            ("/sim/players/0/stats/0/id", serde_json::json!(-1)),
            ("/sim/players/0/buffs/0/uptime", serde_json::json!(101)),
            (
                "/sim/players/0/collected_data/resource_lost/rage/mean",
                serde_json::json!(-1),
            ),
            (
                "/sim/players/0/collected_data/action_sequence/0/time",
                serde_json::json!(-0.1),
            ),
        ];
        for (pointer, replacement) in mutations {
            let mut document: Value = serde_json::from_str(fixture).unwrap();
            *document.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                normalize_quick_result(
                    &serde_json::to_vec(&document).unwrap(),
                    &identity(),
                    "3487fce"
                )
                .is_err(),
                "mutation at {pointer} must fail closed"
            );
        }
    }

    #[test]
    fn bounds_detailed_result_collections() {
        let mut document: Value = serde_json::from_str(include_str!(
            "../../../../test-data/fixtures/reports/quick-1210-01-3487fce.min.json"
        ))
        .unwrap();
        let action = document.pointer("/sim/players/0/stats/0").unwrap().clone();
        document["sim"]["players"][0]["stats"] =
            Value::Array((0..(MAX_ACTIONS + 10)).map(|_| action.clone()).collect());
        let apl = document
            .pointer("/sim/players/0/collected_data/action_sequence/0")
            .unwrap()
            .clone();
        document["sim"]["players"][0]["collected_data"]["action_sequence"] =
            Value::Array((0..(MAX_APL_ACTIONS + 10)).map(|_| apl.clone()).collect());

        let result = normalize_quick_result(
            &serde_json::to_vec(&document).unwrap(),
            &identity(),
            "3487fce",
        )
        .unwrap();
        assert_eq!(result.actions.len(), MAX_ACTIONS);
        assert_eq!(result.apl_sequence.len(), MAX_APL_ACTIONS);
    }

    #[test]
    fn accepts_auto_role_when_simc_emits_an_unambiguous_damage_metric() {
        let mut document: Value = serde_json::from_str(include_str!(
            "../../../../test-data/fixtures/reports/quick-1210-01-3487fce.min.json"
        ))
        .unwrap();
        document["sim"]["players"][0]["role"] = Value::String("auto".into());

        let result = normalize_quick_result(
            &serde_json::to_vec(&document).unwrap(),
            &identity(),
            "3487fce",
        )
        .unwrap();

        assert_eq!(result.player.role, "auto");
        assert_eq!(result.primary_metric.name, "dps");
    }
}

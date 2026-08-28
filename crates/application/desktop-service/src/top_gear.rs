use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use simshredder_domain::{Item, ResultRuntimeIdentity, UpgradeMetadata};
use simshredder_job_runner::{BatchInput, CancellationToken, EnqueueRequest, PersistentQueue};
use simshredder_runtime_manager::RuntimeDoctor;
use simshredder_top_gear::{
    ActionState, BudgetSnapshot, EnhancementPolicy, EvaluatedLoadout, ItemUpgradeMetadata,
    ItemVariant, Loadout, PlannedAction, ProfileOptionVariant, RankedLoadout, RejectionBreakdown,
    RuleManifest, SearchPreview, SearchRequest, TalentVariant, UpgradeMetadataSource, WeaponKind,
    build_action_states, build_profileset_stage_input, build_profileset_target_error_input,
    confidence_survivor_keys, derive_action_plan, generate_loadouts, materialize_upgrade_variants,
    parse_profileset_results, rank_results,
};

use super::{
    DesktopService, Error, ExportView, JobView, QuickSimRequest, Result, cpu_preset,
    prepare_request, read_regular,
};

const MAX_TOP_GEAR_COMBINATIONS: usize = 2_048;
const SESSION_SCHEMA_V1: u32 = 1;
const SESSION_SCHEMA_V2: u32 = 2;

const fn default_low_target_error() -> f64 {
    0.01
}

const fn default_medium_target_error() -> f64 {
    0.002
}

const fn default_high_target_error() -> f64 {
    0.0005
}

const fn default_legacy_low_iterations() -> u32 {
    1_000
}

const fn default_legacy_high_iterations() -> u32 {
    10_000
}

const fn default_legacy_finalist_count() -> usize {
    8
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TopGearRequest {
    pub quick: QuickSimRequest,
    pub variants: Vec<ItemVariant>,
    #[serde(default)]
    pub talent_loadouts: Vec<TalentVariant>,
    #[serde(default)]
    pub profile_options: BTreeMap<String, Vec<ProfileOptionVariant>>,
    #[serde(default)]
    pub locked_slots: std::collections::BTreeSet<simshredder_domain::GearSlot>,
    #[serde(default)]
    pub minimum_set_pieces: BTreeMap<String, u8>,
    #[serde(default)]
    pub catalyst_charges: u8,
    pub balances: BTreeMap<String, u32>,
    pub reserves: BTreeMap<String, u32>,
    pub currency_confirmed_at_unix_seconds: u64,
    #[serde(default)]
    pub enhancement_policy: EnhancementPolicy,
    #[serde(default)]
    pub target_rank_overrides: BTreeMap<String, u8>,
    #[serde(default)]
    pub upgrade_metadata: Option<UpgradeMetadata>,
    #[serde(default)]
    pub upgrade_metadata_confirmed: bool,
    pub rule_revision: String,
    pub game_build: u32,
    pub combination_limit: usize,
    #[serde(default = "default_legacy_low_iterations")]
    pub low_iterations: u32,
    #[serde(default = "default_legacy_high_iterations")]
    pub high_iterations: u32,
    #[serde(default = "default_legacy_finalist_count")]
    pub finalist_count: usize,
    #[serde(default = "default_low_target_error")]
    pub low_target_error: f64,
    #[serde(default = "default_medium_target_error")]
    pub medium_target_error: f64,
    #[serde(default = "default_high_target_error")]
    pub high_target_error: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedTopGear {
    pub profile_name: String,
    pub rule_revision: String,
    pub rule_source: String,
    pub raw_combinations: u64,
    pub valid_combinations: u64,
    pub execution_count: usize,
    pub finalist_count: usize,
    pub estimated: bool,
    pub rejections: RejectionBreakdown,
    pub generated_input: String,
    pub variants: Vec<ItemVariant>,
    pub talent_loadouts: Vec<TalentVariant>,
    pub profile_options: BTreeMap<String, Vec<ProfileOptionVariant>>,
    pub loadouts: Vec<Loadout>,
    pub enhancement_policy: EnhancementPolicy,
    pub upgrade_metadata: Option<UpgradeMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopGearSessionView {
    pub id: String,
    pub stage: String,
    pub current_job: JobView,
    pub low_job_id: i64,
    pub medium_job_id: Option<i64>,
    pub high_job_id: Option<i64>,
    pub action_job_id: Option<i64>,
    pub completed_executions: usize,
    pub total_executions: usize,
    pub can_advance: bool,
    pub pipeline_failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopGearResultView {
    pub session_id: String,
    pub baseline_key: String,
    pub rule_revision: String,
    pub ranked: Vec<RankedLoadout>,
    pub low_job_id: i64,
    pub medium_job_id: Option<i64>,
    pub high_job_id: i64,
    pub action_job_id: Option<i64>,
    pub action_plan: Vec<PlannedAction>,
    pub estimated: bool,
    pub final_generated_input: String,
    pub runtime: ResultRuntimeIdentity,
    pub budget: BudgetSnapshot,
    pub enhancement_policy: EnhancementPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TopGearSession {
    schema_version: u32,
    id: String,
    request: TopGearRequest,
    loadouts: Vec<Loadout>,
    baseline_key: String,
    low_job_id: i64,
    #[serde(default)]
    medium_job_id: Option<i64>,
    #[serde(default)]
    medium_keys: Vec<String>,
    high_job_id: Option<i64>,
    finalist_keys: Vec<String>,
    #[serde(default)]
    action_job_id: Option<i64>,
    #[serde(default)]
    action_states: Vec<ActionState>,
    #[serde(default)]
    action_finalized: bool,
    #[serde(default)]
    estimated: bool,
    #[serde(default)]
    pipeline_failure: Option<String>,
}

pub struct StartedTopGear {
    pub view: TopGearSessionView,
    pub job_id: Option<i64>,
    pub token: Option<CancellationToken>,
}

impl DesktopService {
    pub fn prepare_top_gear(&self, request: &TopGearRequest) -> Result<PreparedTopGear> {
        let (prepared, cpu_plan) = prepare_request(&request.quick)?;
        let rules = bundled_rules()?;
        validate_top_gear_request(request)?;
        let upgrade_metadata = request
            .upgrade_metadata
            .clone()
            .or_else(|| prepared.profile.upgrade_metadata.clone());
        let variants = if request.variants.is_empty() {
            default_variants(&prepared.profile)
        } else {
            request.variants.clone()
        };
        let variants = materialize_upgrade_variants(
            &rules,
            &variants,
            &upgrade_high_watermarks(upgrade_metadata.as_ref()),
        )?;
        let talent_loadouts = if request.talent_loadouts.is_empty() {
            default_talent_loadouts(&prepared.profile)
        } else {
            request.talent_loadouts.clone()
        };
        let profile_options = if request.profile_options.is_empty() {
            default_profile_options(&prepared.profile)
        } else {
            request.profile_options.clone()
        };
        let candidates = group_candidates(&variants, &request.locked_slots);
        let preview = generate_loadouts(
            &rules,
            &SearchRequest {
                expected_rule_revision: request.rule_revision.clone(),
                game_build: request.game_build,
                candidates,
                talent_candidates: talent_loadouts.clone(),
                option_candidates: profile_options.clone(),
                budget: BudgetSnapshot {
                    balances: request.balances.clone(),
                    reserves: request.reserves.clone(),
                    confirmed_at_unix_seconds: request.currency_confirmed_at_unix_seconds,
                },
                enhancement_policy: request.enhancement_policy,
                target_rank_overrides: request.target_rank_overrides.clone(),
                minimum_set_pieces: request.minimum_set_pieces.clone(),
                catalyst_charges: request.catalyst_charges,
                max_combinations: request.combination_limit,
            },
        )?;
        unique_baseline(&preview)?;
        let generated = build_profileset_target_error_input(
            &prepared.generated_bytes,
            &preview.loadouts,
            request.low_target_error,
            Some(cpu_plan.profileset_work_threads),
        )?;
        let finalists = preview.loadouts.len();
        Ok(PreparedTopGear {
            profile_name: prepared.profile.name,
            rule_revision: rules.revision,
            rule_source: rules.source,
            raw_combinations: preview.raw_combinations,
            valid_combinations: preview.valid_combinations,
            execution_count: preview.loadouts.len().saturating_mul(3),
            finalist_count: finalists,
            estimated: preview.was_truncated,
            rejections: preview.rejections,
            generated_input: String::from_utf8(generated).map_err(|_| {
                Error::InvalidRequest("generated Top Gear input is not UTF-8".into())
            })?,
            variants,
            talent_loadouts,
            profile_options,
            loadouts: preview.loadouts,
            enhancement_policy: request.enhancement_policy,
            upgrade_metadata,
        })
    }

    pub fn start_top_gear(
        &self,
        request: &TopGearRequest,
        runtime: &RuntimeDoctor,
    ) -> Result<StartedTopGear> {
        let preview = self.prepare_top_gear(request)?;
        let (prepared, _) = prepare_request(&request.quick)?;
        let baseline_key = preview
            .loadouts
            .iter()
            .find(|loadout| loadout.changed_slots == 0 && loadout.changed_options == 0)
            .ok_or_else(|| Error::InvalidRequest("prepared Top Gear baseline is missing".into()))?
            .key
            .clone();
        let mut queue = self.queue()?;
        let enqueued = enqueue_stage(
            &mut queue,
            &prepared,
            preview.generated_input.as_bytes(),
            request,
            runtime,
        )?;
        let id = new_session_id()?;
        let session = TopGearSession {
            schema_version: SESSION_SCHEMA_V2,
            id: id.clone(),
            request: request.clone(),
            loadouts: preview.loadouts,
            baseline_key,
            low_job_id: enqueued.job_id,
            medium_job_id: None,
            medium_keys: Vec::new(),
            high_job_id: None,
            finalist_keys: Vec::new(),
            action_job_id: None,
            action_states: Vec::new(),
            action_finalized: false,
            estimated: preview.estimated,
            pipeline_failure: None,
        };
        self.write_top_gear_session(&session)?;
        let token = queue.cancel_handle(enqueued.job_id).token();
        Ok(StartedTopGear {
            view: self.top_gear_status(&id)?,
            job_id: Some(enqueued.job_id),
            token: Some(token),
        })
    }

    pub fn top_gear_status(&self, session_id: &str) -> Result<TopGearSessionView> {
        let session = self.read_top_gear_session(session_id)?;
        if session.schema_version == SESSION_SCHEMA_V2 {
            return self.top_gear_status_v2(session);
        }
        let current_job_id = session
            .action_job_id
            .or(session.high_job_id)
            .unwrap_or(session.low_job_id);
        let current_job = self.job(current_job_id)?;
        let low = self.job(session.low_job_id)?;
        let high = session
            .high_job_id
            .map(|job_id| self.job(job_id))
            .transpose()?;
        let action = session
            .action_job_id
            .map(|job_id| self.job(job_id))
            .transpose()?;
        let stage = if session.action_finalized
            || action.as_ref().is_some_and(|job| job.state == "succeeded")
        {
            "complete"
        } else if session.action_job_id.is_some() {
            "action_plan"
        } else if session.high_job_id.is_some() {
            "high_precision"
        } else {
            "low_precision"
        };
        let low_count = if low.state == "succeeded" {
            session.loadouts.len()
        } else {
            0
        };
        let high_count = if high.as_ref().is_some_and(|job| job.state == "succeeded") {
            session.finalist_keys.len()
        } else {
            0
        };
        let action_count = if action.as_ref().is_some_and(|job| job.state == "succeeded") {
            session.action_states.len()
        } else {
            0
        };
        let can_advance = if session.high_job_id.is_none() {
            low.state == "succeeded"
        } else {
            session.action_job_id.is_none()
                && !session.action_finalized
                && high.as_ref().is_some_and(|job| job.state == "succeeded")
        };
        Ok(TopGearSessionView {
            id: session.id,
            stage: stage.into(),
            current_job,
            low_job_id: session.low_job_id,
            medium_job_id: session.medium_job_id,
            high_job_id: session.high_job_id,
            action_job_id: session.action_job_id,
            completed_executions: low_count + high_count + action_count,
            total_executions: session.loadouts.len()
                + session
                    .request
                    .finalist_count
                    .min(session.loadouts.len())
                    .max(1)
                + session.action_states.len(),
            can_advance,
            pipeline_failure: session.pipeline_failure,
        })
    }

    fn top_gear_status_v2(&self, session: TopGearSession) -> Result<TopGearSessionView> {
        let low = self.job(session.low_job_id)?;
        let medium = session
            .medium_job_id
            .map(|job_id| self.job(job_id))
            .transpose()?;
        let high = session
            .high_job_id
            .map(|job_id| self.job(job_id))
            .transpose()?;
        let (stage, current_job) = if let Some(high) = high.as_ref() {
            (
                if high.state == "succeeded" {
                    "complete"
                } else {
                    "high_precision"
                },
                high.clone(),
            )
        } else if let Some(medium) = medium.as_ref() {
            ("medium_precision", medium.clone())
        } else {
            ("low_precision", low.clone())
        };
        let low_count = usize::from(low.state == "succeeded") * session.loadouts.len();
        let medium_count = usize::from(medium.as_ref().is_some_and(|job| job.state == "succeeded"))
            * session.medium_keys.len();
        let high_count = usize::from(high.as_ref().is_some_and(|job| job.state == "succeeded"))
            * session.finalist_keys.len();
        Ok(TopGearSessionView {
            id: session.id,
            stage: stage.into(),
            current_job,
            low_job_id: session.low_job_id,
            medium_job_id: session.medium_job_id,
            high_job_id: session.high_job_id,
            action_job_id: session.action_job_id,
            completed_executions: low_count + medium_count + high_count,
            total_executions: session.loadouts.len()
                + session.medium_keys.len()
                + session.finalist_keys.len(),
            can_advance: false,
            pipeline_failure: session.pipeline_failure,
        })
    }

    pub fn top_gear_sessions(&self) -> Result<Vec<TopGearSessionView>> {
        let root = self.top_gear_root();
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("json")
                && let Some(id) = path.file_stem().and_then(|value| value.to_str())
            {
                ids.push(id.to_owned());
            }
        }
        ids.sort();
        ids.into_iter()
            .map(|id| self.top_gear_status(&id))
            .collect()
    }

    pub fn delete_top_gear_session(&self, session_id: &str) -> Result<()> {
        let session = self.read_top_gear_session(session_id)?;
        let path = self.session_path(session_id)?;
        let tombstone = path.with_extension(format!("json.deleted-{}", std::process::id()));
        if fs::symlink_metadata(&tombstone).is_ok() {
            return Err(Error::InvalidRequest(
                "Top Gear session deletion is already staged".into(),
            ));
        }
        fs::rename(&path, &tombstone)?;
        let job_ids = [
            Some(session.low_job_id),
            session.medium_job_id,
            session.high_job_id,
            session.action_job_id,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if let Err(error) = self.delete_jobs(&job_ids) {
            let _ = fs::rename(&tombstone, &path);
            return Err(error);
        }
        fs::remove_file(tombstone)?;
        Ok(())
    }

    pub fn advance_top_gear(
        &self,
        session_id: &str,
        runtime: &RuntimeDoctor,
    ) -> Result<StartedTopGear> {
        let mut session = self.read_top_gear_session(session_id)?;
        if session.schema_version == SESSION_SCHEMA_V2 {
            let result = self.advance_top_gear_v2(&mut session, runtime);
            if let Err(error) = &result {
                session.pipeline_failure = Some(error.to_string());
                self.write_top_gear_session(&session)?;
            }
            return result;
        }
        if let Some(high_job_id) = session.high_job_id {
            if session.action_job_id.is_some() || session.action_finalized {
                return Err(Error::InvalidRequest(
                    "Top Gear action stage already exists".into(),
                ));
            }
            if self.job(high_job_id)?.state != "succeeded" {
                return Err(Error::InvalidRequest(
                    "high-precision stage is not complete".into(),
                ));
            }
            let high_results = self.profileset_results(high_job_id)?;
            let evaluations = match_evaluations(&session.loadouts, &high_results)?;
            let baseline = evaluations
                .iter()
                .find(|entry| entry.loadout.key == session.baseline_key)
                .ok_or_else(|| Error::InvalidRequest("baseline result is missing".into()))?;
            let ranked = rank_results(baseline.mean, baseline.mean_error, evaluations)?;
            let winner = &ranked
                .first()
                .ok_or_else(|| Error::InvalidRequest("finalist result is empty".into()))?
                .loadout;
            let baseline_loadout = session
                .loadouts
                .iter()
                .find(|loadout| loadout.key == session.baseline_key)
                .ok_or_else(|| Error::InvalidRequest("baseline loadout is missing".into()))?;
            let states = build_action_states(baseline_loadout, winner)?;
            if winner.items.values().all(|item| item.actions.is_empty()) {
                session.action_finalized = true;
                self.write_top_gear_session(&session)?;
                return Ok(StartedTopGear {
                    view: self.top_gear_status(session_id)?,
                    job_id: None,
                    token: None,
                });
            }
            let (prepared, cpu_plan) = prepare_request(&session.request.quick)?;
            let state_loadouts: Vec<_> = states.iter().map(|state| state.loadout.clone()).collect();
            let generated = build_profileset_stage_input(
                &prepared.generated_bytes,
                &state_loadouts,
                Some(session.request.high_iterations),
                Some(cpu_plan.profileset_work_threads),
            )?;
            let mut queue = self.queue()?;
            let enqueued =
                enqueue_stage(&mut queue, &prepared, &generated, &session.request, runtime)?;
            session.action_job_id = Some(enqueued.job_id);
            session.action_states = states;
            self.write_top_gear_session(&session)?;
            let token = queue.cancel_handle(enqueued.job_id).token();
            return Ok(StartedTopGear {
                view: self.top_gear_status(session_id)?,
                job_id: Some(enqueued.job_id),
                token: Some(token),
            });
        }
        if self.job(session.low_job_id)?.state != "succeeded" {
            return Err(Error::InvalidRequest(
                "low-precision stage is not complete".into(),
            ));
        }
        let low_results = self.profileset_results(session.low_job_id)?;
        let mut evaluations = match_evaluations(&session.loadouts, &low_results)?;
        evaluations.sort_by(|left, right| right.mean.total_cmp(&left.mean));
        let mut finalist_keys = vec![session.baseline_key.clone()];
        for evaluation in evaluations {
            if finalist_keys.len()
                >= session
                    .request
                    .finalist_count
                    .min(session.loadouts.len())
                    .max(1)
            {
                break;
            }
            if !finalist_keys.contains(&evaluation.loadout.key) {
                finalist_keys.push(evaluation.loadout.key);
            }
        }
        let finalists: Vec<_> = finalist_keys
            .iter()
            .filter_map(|key| session.loadouts.iter().find(|loadout| &loadout.key == key))
            .cloned()
            .collect();
        let (prepared, cpu_plan) = prepare_request(&session.request.quick)?;
        let generated = build_profileset_stage_input(
            &prepared.generated_bytes,
            &finalists,
            Some(session.request.high_iterations),
            Some(cpu_plan.profileset_work_threads),
        )?;
        let mut queue = self.queue()?;
        let enqueued = enqueue_stage(&mut queue, &prepared, &generated, &session.request, runtime)?;
        session.high_job_id = Some(enqueued.job_id);
        session.finalist_keys = finalist_keys;
        self.write_top_gear_session(&session)?;
        let token = queue.cancel_handle(enqueued.job_id).token();
        Ok(StartedTopGear {
            view: self.top_gear_status(session_id)?,
            job_id: Some(enqueued.job_id),
            token: Some(token),
        })
    }

    fn advance_top_gear_v2(
        &self,
        session: &mut TopGearSession,
        runtime: &RuntimeDoctor,
    ) -> Result<StartedTopGear> {
        if session.high_job_id.is_some() {
            return Ok(StartedTopGear {
                view: self.top_gear_status(&session.id)?,
                job_id: None,
                token: None,
            });
        }
        let (source_job_id, source_keys, target_error, next_stage) =
            if let Some(medium_job_id) = session.medium_job_id {
                (
                    medium_job_id,
                    session.medium_keys.clone(),
                    session.request.high_target_error,
                    "high",
                )
            } else {
                (
                    session.low_job_id,
                    session
                        .loadouts
                        .iter()
                        .map(|loadout| loadout.key.clone())
                        .collect(),
                    session.request.medium_target_error,
                    "medium",
                )
            };
        let source_job = self.job(source_job_id)?;
        if source_job.state != "succeeded" {
            return Ok(StartedTopGear {
                view: self.top_gear_status(&session.id)?,
                job_id: None,
                token: None,
            });
        }
        let source_loadouts = loadouts_for_keys(&session.loadouts, &source_keys)?;
        let evaluations =
            match_evaluations(&source_loadouts, &self.profileset_results(source_job_id)?)?;
        let survivor_keys = confidence_survivor_keys(&evaluations, &session.baseline_key)?;
        let survivors = loadouts_for_keys(&session.loadouts, &survivor_keys)?;
        let (prepared, cpu_plan) = prepare_request(&session.request.quick)?;
        let generated = build_profileset_target_error_input(
            &prepared.generated_bytes,
            &survivors,
            target_error,
            Some(cpu_plan.profileset_work_threads),
        )?;
        let mut queue = self.queue()?;
        let enqueued = enqueue_stage(&mut queue, &prepared, &generated, &session.request, runtime)?;
        if next_stage == "medium" {
            session.medium_job_id = Some(enqueued.job_id);
            session.medium_keys = survivor_keys;
        } else {
            session.high_job_id = Some(enqueued.job_id);
            session.finalist_keys = survivor_keys;
        }
        session.pipeline_failure = None;
        self.write_top_gear_session(session)?;
        let token = queue.cancel_handle(enqueued.job_id).token();
        Ok(StartedTopGear {
            view: self.top_gear_status(&session.id)?,
            job_id: Some(enqueued.job_id),
            token: Some(token),
        })
    }

    pub fn top_gear_result(&self, session_id: &str) -> Result<TopGearResultView> {
        let session = self.read_top_gear_session(session_id)?;
        let high_job_id = session
            .high_job_id
            .ok_or_else(|| Error::InvalidRequest("high-precision stage has not started".into()))?;
        if self.job(high_job_id)?.state != "succeeded" {
            return Err(Error::ResultUnavailable(high_job_id));
        }
        let results = self.profileset_results(high_job_id)?;
        let evaluations = match_evaluations(&session.loadouts, &results)?;
        let baseline = evaluations
            .iter()
            .find(|entry| entry.loadout.key == session.baseline_key)
            .ok_or_else(|| Error::InvalidRequest("baseline result is missing".into()))?;
        let ranked = rank_results(baseline.mean, baseline.mean_error, evaluations)?;
        let winner = &ranked
            .first()
            .ok_or_else(|| Error::InvalidRequest("finalist result is empty".into()))?
            .loadout;
        let action_plan = if session.schema_version == SESSION_SCHEMA_V1
            && let Some(action_job_id) = session.action_job_id
        {
            if self.job(action_job_id)?.state != "succeeded" {
                return Err(Error::ResultUnavailable(action_job_id));
            }
            let actions: Vec<_> = winner
                .items
                .values()
                .flat_map(|item| item.actions.iter().cloned())
                .collect();
            derive_action_plan(
                &actions,
                &session.action_states,
                &self.profileset_results(action_job_id)?,
                &BudgetSnapshot {
                    balances: session.request.balances.clone(),
                    reserves: session.request.reserves.clone(),
                    confirmed_at_unix_seconds: session.request.currency_confirmed_at_unix_seconds,
                },
            )?
        } else {
            Vec::new()
        };
        let mut final_prepared = prepare_request(&session.request.quick)?.0;
        if session.schema_version == SESSION_SCHEMA_V2 {
            final_prepared.profile.scalar_options.insert(
                "target_error".into(),
                format!("{:.6}", session.request.high_target_error),
            );
        } else {
            final_prepared.profile.simulation.iterations = session.request.high_iterations;
        }
        if !winner.talent.option.is_empty() {
            final_prepared
                .profile
                .talents
                .insert(winner.talent.option.clone(), winner.talent.value.clone());
        }
        for candidate in winner.profile_options.values() {
            if candidate.value.is_empty() {
                continue;
            }
            if candidate.option == "omnium_talents" {
                final_prepared
                    .profile
                    .talents
                    .insert(candidate.option.clone(), candidate.value.clone());
            } else {
                final_prepared
                    .profile
                    .scalar_options
                    .insert(candidate.option.clone(), candidate.value.clone());
            }
        }
        final_prepared.profile.equipped = winner
            .items
            .iter()
            .map(|(slot, variant)| {
                (
                    *slot,
                    Item {
                        slot: *slot,
                        id: variant.source_item_id,
                        name: variant.display_name.clone(),
                        options: variant.simc_options.clone(),
                    },
                )
            })
            .collect();
        let final_prepared = simshredder_core::prepare_profile(
            &final_prepared.source_bytes,
            final_prepared.profile,
        )?;
        let high_result = self.result(high_job_id)?;
        Ok(TopGearResultView {
            session_id: session.id,
            baseline_key: session.baseline_key,
            rule_revision: session.request.rule_revision,
            ranked,
            low_job_id: session.low_job_id,
            medium_job_id: session.medium_job_id,
            high_job_id,
            action_job_id: session.action_job_id,
            action_plan,
            estimated: session.estimated,
            final_generated_input: String::from_utf8(final_prepared.generated_bytes)
                .map_err(|_| Error::InvalidRequest("final Top Gear input is not UTF-8".into()))?,
            runtime: high_result.result.runtime,
            budget: BudgetSnapshot {
                balances: session.request.balances,
                reserves: session.request.reserves,
                confirmed_at_unix_seconds: session.request.currency_confirmed_at_unix_seconds,
            },
            enhancement_policy: session.request.enhancement_policy,
        })
    }

    pub fn export_top_gear(&self, session_id: &str, destination_root: &Path) -> Result<ExportView> {
        let session = self.read_top_gear_session(session_id)?;
        let result = self.top_gear_result(session_id)?;
        let low = self.result(session.low_job_id)?;
        let high = self.result(result.high_job_id)?;
        let mut files = vec![
            (
                "final.simc".to_owned(),
                result.final_generated_input.as_bytes().to_vec(),
            ),
            ("plan.json".to_owned(), serde_json::to_vec_pretty(&result)?),
            ("low-result.json".to_owned(), low.raw_json.into_bytes()),
            ("high-result.json".to_owned(), high.raw_json.into_bytes()),
            ("high-report.html".to_owned(), high.raw_html.into_bytes()),
        ];
        if let Some(medium_job_id) = result.medium_job_id {
            let medium = self.result(medium_job_id)?;
            files.push((
                "medium-result.json".to_owned(),
                medium.raw_json.into_bytes(),
            ));
        }
        if let Some(action_job_id) = result.action_job_id {
            let action = self.result(action_job_id)?;
            files.push((
                "action-result.json".to_owned(),
                action.raw_json.into_bytes(),
            ));
        }
        fs::create_dir_all(destination_root)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InvalidRequest("system clock is before the Unix epoch".into()))?
            .as_millis();
        let destination = destination_root.join(format!("top-gear-{session_id}-{timestamp}"));
        let staging = destination_root.join(format!(
            ".top-gear-{session_id}-{timestamp}-{}.staging",
            std::process::id()
        ));
        fs::create_dir(&staging)?;
        super::protect_directory(&staging)?;
        for (name, bytes) in &files {
            let path = staging.join(name);
            fs::write(&path, bytes)?;
            super::protect_file(&path)?;
        }
        fs::rename(staging, &destination)?;
        Ok(ExportView {
            directory: destination,
            file_count: files.len(),
        })
    }

    fn profileset_results(
        &self,
        job_id: i64,
    ) -> Result<Vec<simshredder_top_gear::ProfilesetResult>> {
        let result = self.result(job_id)?;
        let document: serde_json::Value = serde_json::from_str(&result.raw_json)?;
        Ok(parse_profileset_results(&document)?)
    }

    fn top_gear_root(&self) -> PathBuf {
        self.run_root
            .parent()
            .unwrap_or(&self.run_root)
            .join("top-gear-sessions")
    }

    fn session_path(&self, session_id: &str) -> Result<PathBuf> {
        if session_id.is_empty()
            || session_id.len() > 80
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(Error::InvalidRequest("unsafe Top Gear session id".into()));
        }
        Ok(self.top_gear_root().join(format!("{session_id}.json")))
    }

    fn read_top_gear_session(&self, session_id: &str) -> Result<TopGearSession> {
        let path = self.session_path(session_id)?;
        let session: TopGearSession = serde_json::from_slice(&read_regular(&path)?)?;
        if !matches!(
            session.schema_version,
            SESSION_SCHEMA_V1 | SESSION_SCHEMA_V2
        ) || session.id != session_id
        {
            return Err(Error::InvalidRequest(
                "Top Gear session identity is invalid".into(),
            ));
        }
        Ok(session)
    }

    fn write_top_gear_session(&self, session: &TopGearSession) -> Result<()> {
        let root = self.top_gear_root();
        fs::create_dir_all(&root)?;
        super::protect_directory(&root)?;
        let path = self.session_path(&session.id)?;
        let staging = path.with_extension(format!("json.{}.staging", std::process::id()));
        let mut bytes = serde_json::to_vec_pretty(session)?;
        bytes.push(b'\n');
        fs::write(&staging, bytes)?;
        super::protect_file(&staging)?;
        fs::rename(staging, path)?;
        Ok(())
    }
}

fn bundled_rules() -> Result<RuleManifest> {
    Ok(serde_json::from_str(include_str!(
        "../../../../resources/rules/12.1.0-69465-v1.json"
    ))?)
}

fn validate_top_gear_request(request: &TopGearRequest) -> Result<()> {
    if !(1..=MAX_TOP_GEAR_COMBINATIONS).contains(&request.combination_limit) {
        return Err(Error::InvalidRequest(format!(
            "combination limit must be between 1 and {MAX_TOP_GEAR_COMBINATIONS}"
        )));
    }
    if !request.low_target_error.is_finite()
        || !request.medium_target_error.is_finite()
        || !request.high_target_error.is_finite()
        || !(0.000_001..=0.1).contains(&request.high_target_error)
        || request.high_target_error >= request.medium_target_error
        || request.medium_target_error >= request.low_target_error
        || request.low_target_error > 0.1
    {
        return Err(Error::InvalidRequest(
            "Top Gear target-error stages must strictly increase in precision".into(),
        ));
    }
    Ok(())
}

fn upgrade_high_watermarks(
    metadata: Option<&UpgradeMetadata>,
) -> BTreeMap<simshredder_domain::GearSlot, simshredder_top_gear::SlotHighWatermark> {
    metadata
        .into_iter()
        .flat_map(|metadata| metadata.slot_high_watermarks.values())
        .filter_map(|watermark| {
            let slot = match watermark.slot_index {
                1 => simshredder_domain::GearSlot::Head,
                2 => simshredder_domain::GearSlot::Neck,
                3 => simshredder_domain::GearSlot::Shoulders,
                5 => simshredder_domain::GearSlot::Chest,
                6 => simshredder_domain::GearSlot::Waist,
                7 => simshredder_domain::GearSlot::Legs,
                8 => simshredder_domain::GearSlot::Feet,
                9 => simshredder_domain::GearSlot::Wrists,
                10 => simshredder_domain::GearSlot::Hands,
                11 => simshredder_domain::GearSlot::Finger1,
                12 => simshredder_domain::GearSlot::Finger2,
                13 => simshredder_domain::GearSlot::Trinket1,
                14 => simshredder_domain::GearSlot::Trinket2,
                15 => simshredder_domain::GearSlot::Back,
                16 => simshredder_domain::GearSlot::MainHand,
                17 => simshredder_domain::GearSlot::OffHand,
                _ => return None,
            };
            Some((
                slot,
                simshredder_top_gear::SlotHighWatermark {
                    character_item_level: watermark.character_item_level,
                    account_item_level: watermark.account_item_level,
                },
            ))
        })
        .collect()
}

fn default_variants(profile: &simshredder_domain::Profile) -> Vec<ItemVariant> {
    let mut variants = Vec::new();
    for (slot, item) in &profile.equipped {
        let owned_item_key = format!("worn-{}-{}", slot.simc_token(), item.id);
        variants.push(ItemVariant {
            key: owned_item_key.clone(),
            source_item_id: item.id,
            slot: *slot,
            display_name: item.name.clone(),
            rank: 0,
            gem_ids: parse_gems(item.options.get("gem_id")),
            enchant_id: item
                .options
                .get("enchant_id")
                .and_then(|value| value.parse().ok()),
            simc_options: item.options.clone(),
            cost: BTreeMap::new(),
            upgrade: ItemUpgradeMetadata {
                owned_item_key,
                current_rank: 0,
                max_rank: None,
                track: None,
                source: UpgradeMetadataSource::Unknown,
            },
            actions: Vec::new(),
            unique_groups: Default::default(),
            set_groups: Default::default(),
            weapon_kind: WeaponKind::None,
            embellishment: false,
            catalyst: false,
            enabled: true,
            changed: false,
        });
    }
    for (index, bag) in profile.bag_items.iter().enumerate() {
        let owned_item_key = format!("bag-{}-{}-{index}", bag.item.slot.simc_token(), bag.item.id);
        variants.push(ItemVariant {
            key: owned_item_key.clone(),
            source_item_id: bag.item.id,
            slot: bag.item.slot,
            display_name: bag.item.name.clone().or_else(|| bag.name.clone()),
            rank: 0,
            gem_ids: parse_gems(bag.item.options.get("gem_id")),
            enchant_id: bag
                .item
                .options
                .get("enchant_id")
                .and_then(|value| value.parse().ok()),
            simc_options: bag.item.options.clone(),
            cost: BTreeMap::new(),
            upgrade: ItemUpgradeMetadata {
                owned_item_key,
                current_rank: 0,
                max_rank: None,
                track: None,
                source: UpgradeMetadataSource::Unknown,
            },
            actions: Vec::new(),
            unique_groups: Default::default(),
            set_groups: Default::default(),
            weapon_kind: WeaponKind::None,
            embellishment: false,
            catalyst: false,
            enabled: true,
            changed: true,
        });
    }
    variants
}

fn parse_gems(value: Option<&String>) -> Vec<u32> {
    value
        .into_iter()
        .flat_map(|value| value.split('/'))
        .filter_map(|value| value.parse().ok())
        .collect()
}

fn default_talent_loadouts(profile: &simshredder_domain::Profile) -> Vec<TalentVariant> {
    let (option, value) = profile
        .talents
        .iter()
        .find(|(key, _)| key.as_str() == "talents")
        .or_else(|| profile.talents.iter().next())
        .map(|(key, value)| (key.clone(), value.clone()))
        .unwrap_or_default();
    let mut loadouts = vec![TalentVariant {
        key: "active".into(),
        label: "Active".into(),
        option: option.clone(),
        value: value.clone(),
        changed: false,
        enabled: true,
    }];
    for (index, saved) in profile.saved_talent_loadouts.iter().enumerate() {
        if saved.value != value {
            loadouts.push(TalentVariant {
                key: format!("saved-{index}"),
                label: saved.name.clone(),
                option: option.clone(),
                value: saved.value.clone(),
                changed: true,
                enabled: false,
            });
        }
    }
    loadouts
}

fn default_profile_options(
    profile: &simshredder_domain::Profile,
) -> BTreeMap<String, Vec<ProfileOptionVariant>> {
    [
        "food",
        "flask",
        "potion",
        "augmentation",
        "temporary_enchant",
        "omnium_talents",
    ]
    .into_iter()
    .map(|option| {
        let value = if option == "omnium_talents" {
            profile.talents.get(option)
        } else {
            profile.scalar_options.get(option)
        }
        .cloned()
        .unwrap_or_default();
        (
            option.to_owned(),
            vec![ProfileOptionVariant {
                key: format!("active-{option}"),
                label: "Active".into(),
                option: option.to_owned(),
                value,
                changed: false,
                enabled: true,
            }],
        )
    })
    .collect()
}

fn group_candidates(
    variants: &[ItemVariant],
    locked_slots: &std::collections::BTreeSet<simshredder_domain::GearSlot>,
) -> BTreeMap<simshredder_domain::GearSlot, Vec<ItemVariant>> {
    let mut grouped = BTreeMap::new();
    for variant in variants {
        if !variant.enabled {
            continue;
        }
        let pooled_slots = match variant.slot {
            simshredder_domain::GearSlot::Finger1 | simshredder_domain::GearSlot::Finger2
                if variant.changed =>
            {
                Some([
                    simshredder_domain::GearSlot::Finger1,
                    simshredder_domain::GearSlot::Finger2,
                ])
            }
            simshredder_domain::GearSlot::Trinket1 | simshredder_domain::GearSlot::Trinket2
                if variant.changed =>
            {
                Some([
                    simshredder_domain::GearSlot::Trinket1,
                    simshredder_domain::GearSlot::Trinket2,
                ])
            }
            _ => None,
        };
        let targets = pooled_slots
            .map(|slots| slots.to_vec())
            .unwrap_or_else(|| vec![variant.slot]);
        for target in targets {
            if locked_slots.contains(&target) && variant.changed {
                continue;
            }
            let mut candidate = variant.clone();
            candidate.slot = target;
            for action in &mut candidate.actions {
                action.slot = target;
            }
            grouped
                .entry(target)
                .or_insert_with(Vec::new)
                .push(candidate);
        }
    }
    grouped
}

fn unique_baseline(preview: &SearchPreview) -> Result<&Loadout> {
    let mut baseline = preview
        .loadouts
        .iter()
        .filter(|loadout| loadout.changed_slots == 0 && loadout.changed_options == 0);
    let first = baseline.next().ok_or_else(|| {
        Error::InvalidRequest("Top Gear candidates must include the worn baseline".into())
    })?;
    if baseline.next().is_some() {
        return Err(Error::InvalidRequest(
            "Top Gear candidates contain multiple baselines".into(),
        ));
    }
    Ok(first)
}

fn enqueue_stage(
    queue: &mut PersistentQueue,
    prepared: &simshredder_core::PreparedRun,
    generated: &[u8],
    request: &TopGearRequest,
    runtime: &RuntimeDoctor,
) -> Result<simshredder_job_runner::EnqueuedJob> {
    let cache_revision = format!(
        "{}|policy={:?}|balances={:?}|reserves={:?}|targets={:?}",
        request.rule_revision,
        request.enhancement_policy,
        request.balances,
        request.reserves,
        request.target_rank_overrides
    );
    Ok(queue.enqueue(EnqueueRequest {
        profile: &prepared.profile,
        batches: vec![BatchInput {
            source_kind: prepared.profile.source_kind,
            source_bytes: prepared.source_bytes.clone(),
            generated_bytes: generated.to_vec(),
        }],
        executable: &runtime.executable,
        expected_revision: &runtime.record.build,
        cpu_preset: cpu_preset(request.quick.cpu_preset),
        timeout: Duration::from_secs(60 * 60),
        rule_revision: &cache_revision,
    })?)
}

fn match_evaluations(
    loadouts: &[Loadout],
    results: &[simshredder_top_gear::ProfilesetResult],
) -> Result<Vec<EvaluatedLoadout>> {
    results
        .iter()
        .map(|result| {
            let loadout = loadouts
                .iter()
                .find(|loadout| loadout.key == result.key)
                .cloned()
                .ok_or_else(|| {
                    Error::InvalidRequest(format!("unknown profileset result {}", result.key))
                })?;
            Ok(EvaluatedLoadout {
                loadout,
                mean: result.mean,
                mean_error: result.mean_error,
            })
        })
        .collect()
}

fn loadouts_for_keys(loadouts: &[Loadout], keys: &[String]) -> Result<Vec<Loadout>> {
    keys.iter()
        .map(|key| {
            loadouts
                .iter()
                .find(|loadout| &loadout.key == key)
                .cloned()
                .ok_or_else(|| Error::InvalidRequest(format!("unknown Top Gear loadout key {key}")))
        })
        .collect()
}

fn new_session_id() -> Result<String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidRequest("system clock is before the Unix epoch".into()))?
        .as_millis();
    Ok(format!("tg-{timestamp}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use simshredder_domain::GearSlot;
    use simshredder_top_gear::WeaponKind;

    use super::*;
    use crate::{CpuChoice, SourceFormat};

    fn request() -> TopGearRequest {
        let item = |key: &str, changed: bool| ItemVariant {
            key: key.into(),
            source_item_id: 154029,
            slot: GearSlot::Head,
            display_name: None,
            rank: 1,
            gem_ids: Vec::new(),
            enchant_id: None,
            simc_options: if changed {
                BTreeMap::from([("bonus_id".into(), "100/200".into())])
            } else {
                BTreeMap::new()
            },
            cost: BTreeMap::from([("crest".into(), u32::from(changed) * 2)]),
            upgrade: ItemUpgradeMetadata {
                owned_item_key: key.into(),
                current_rank: 1,
                max_rank: Some(1),
                track: None,
                source: UpgradeMetadataSource::Manual,
            },
            actions: Vec::new(),
            unique_groups: BTreeSet::new(),
            set_groups: BTreeSet::new(),
            weapon_kind: WeaponKind::None,
            embellishment: false,
            catalyst: false,
            enabled: true,
            changed,
        };
        TopGearRequest {
            quick: QuickSimRequest {
                source: "warrior=Core\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nhead=,id=154029\nload_default_gear=1\n".into(),
                format: SourceFormat::SimcFile,
                iterations: 100,
                fixed_time: true,
                max_time_seconds: 30,
                vary_combat_length: 0.0,
                desired_targets: 1,
                fight_style: "Patchwerk".into(),
                cpu_preset: CpuChoice::Balanced,
                analysis: crate::AnalysisOptions::default(),
            },
            variants: vec![item("worn", false), item("upgraded", true)],
            talent_loadouts: Vec::new(),
            profile_options: BTreeMap::new(),
            locked_slots: BTreeSet::new(),
            minimum_set_pieces: BTreeMap::new(),
            catalyst_charges: 0,
            balances: BTreeMap::from([("crest".into(), 10)]),
            reserves: BTreeMap::from([("crest".into(), 2)]),
            currency_confirmed_at_unix_seconds: 1,
            enhancement_policy: EnhancementPolicy::MaxPotential,
            target_rank_overrides: BTreeMap::new(),
            upgrade_metadata: None,
            upgrade_metadata_confirmed: false,
            rule_revision: "12.1.0-69465-v1".into(),
            game_build: 69465,
            combination_limit: 16,
            low_iterations: 100,
            high_iterations: 200,
            finalist_count: 2,
            low_target_error: default_low_target_error(),
            medium_target_error: default_medium_target_error(),
            high_target_error: default_high_target_error(),
        }
    }

    #[test]
    fn preview_and_execution_counts_share_the_same_iterator() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let preview = service.prepare_top_gear(&request()).unwrap();
        assert_eq!(preview.raw_combinations, 2);
        assert_eq!(preview.valid_combinations, 2);
        assert_eq!(preview.execution_count, 6);
        assert!(preview.generated_input.contains("profileset."));
        assert!(preview.generated_input.contains("target_error=0.010000\n"));
    }

    #[test]
    fn addon_style_comments_populate_named_items_and_saved_talent_dimensions() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut request = request();
        request.quick.source = "rogue=Character\nlevel=90\nrace=void_elf\nrole=melee\nspec=subtlety\ntalents=ACTIVE\n# Saved Loadout: Dungeon\n# talents=SAVED\n# Named Helm (289)\nhead=,id=250006,enchant_id=8017,context=35\n".into();
        request.variants.clear();
        request.talent_loadouts.clear();
        request.profile_options.clear();
        let preview = service.prepare_top_gear(&request).unwrap();
        assert_eq!(
            preview.variants[0].display_name.as_deref(),
            Some("Named Helm")
        );
        assert_eq!(preview.talent_loadouts.len(), 2);
        assert_eq!(preview.talent_loadouts[1].label, "Dungeon");
        assert!(!preview.talent_loadouts[1].enabled);
        assert_eq!(preview.raw_combinations, 1);
        assert_eq!(preview.loadouts.len(), 1);
        assert_eq!(preview.loadouts[0].changed_slots, 0);
    }

    #[test]
    fn one_bag_trinket_candidate_can_fill_either_trinket_position() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut request = request();
        request.quick.source = "rogue=Character\nlevel=90\nrace=void_elf\nrole=melee\nspec=subtlety\ntrinket1=,id=1001\ntrinket2=,id=1002\n### Gear from Bags\n# Candidate Trinket (334)\n# trinket1=candidate_trinket,id=2001,ilevel=334\n".into();
        request.variants.clear();
        request.talent_loadouts.clear();
        request.profile_options.clear();

        let preview = service.prepare_top_gear(&request).unwrap();
        assert_eq!(preview.raw_combinations, 4);
        assert_eq!(preview.valid_combinations, 3);
        assert_eq!(preview.rejections.unique_equipped, 1);

        let pairs: BTreeSet<_> = preview
            .loadouts
            .iter()
            .map(|loadout| {
                let mut ids: Vec<_> = loadout
                    .items
                    .values()
                    .map(|item| item.source_item_id)
                    .collect();
                ids.sort_unstable();
                ids
            })
            .collect();
        assert_eq!(
            pairs,
            BTreeSet::from([vec![1001, 1002], vec![1001, 2001], vec![1002, 2001]])
        );
    }

    #[test]
    fn twenty_nine_bag_trinkets_fit_the_default_complete_search_cap() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut request = request();
        let mut source = "rogue=Character\nlevel=90\nrace=void_elf\nrole=melee\nspec=subtlety\ntrinket1=,id=1001\ntrinket2=,id=1002\n### Gear from Bags\n".to_owned();
        for index in 0..29 {
            source.push_str(&format!(
                "# Candidate {index} (334)\n# trinket1=candidate_{index},id={},ilevel=334\n",
                2001 + index
            ));
        }
        request.quick.source = source;
        request.variants.clear();
        request.talent_loadouts.clear();
        request.profile_options.clear();
        request.combination_limit = 1_024;

        let preview = service.prepare_top_gear(&request).unwrap();
        assert_eq!(preview.raw_combinations, 900);
        assert_eq!(preview.valid_combinations, 465);
        assert_eq!(preview.loadouts.len(), 465);
        assert_eq!(preview.rejections.unique_equipped, 29);
        assert_eq!(preview.rejections.symmetric_duplicate, 406);
        assert!(!preview.estimated);
    }

    #[test]
    fn stale_rule_blocks_top_gear_without_affecting_quick_prepare() {
        let temporary = tempfile::tempdir().unwrap();
        let service = DesktopService::open(temporary.path()).unwrap();
        let mut stale = request();
        stale.rule_revision = "stale".into();
        assert!(service.prepare_top_gear(&stale).is_err());
        assert!(service.prepare(&stale.quick).is_ok());
    }

    #[test]
    fn legacy_session_without_adaptive_or_upgrade_fields_remains_readable() {
        let session = TopGearSession {
            schema_version: SESSION_SCHEMA_V1,
            id: "legacy-session".into(),
            request: request(),
            loadouts: Vec::new(),
            baseline_key: "baseline".into(),
            low_job_id: 1,
            medium_job_id: None,
            medium_keys: Vec::new(),
            high_job_id: Some(2),
            finalist_keys: Vec::new(),
            action_job_id: Some(3),
            action_states: Vec::new(),
            action_finalized: true,
            estimated: false,
            pipeline_failure: None,
        };
        let mut document = serde_json::to_value(session).unwrap();
        let object = document.as_object_mut().unwrap();
        object.remove("mediumJobId");
        object.remove("mediumKeys");
        object.remove("pipelineFailure");
        let request = object.get_mut("request").unwrap().as_object_mut().unwrap();
        for key in [
            "enhancementPolicy",
            "targetRankOverrides",
            "upgradeMetadata",
            "upgradeMetadataConfirmed",
            "lowIterations",
            "highIterations",
            "finalistCount",
            "lowTargetError",
            "mediumTargetError",
            "highTargetError",
        ] {
            request.remove(key);
        }
        for variant in request.get_mut("variants").unwrap().as_array_mut().unwrap() {
            variant.as_object_mut().unwrap().remove("upgrade");
        }

        let restored: TopGearSession = serde_json::from_value(document).unwrap();
        assert_eq!(restored.schema_version, SESSION_SCHEMA_V1);
        assert_eq!(restored.medium_job_id, None);
        assert_eq!(restored.pipeline_failure, None);
        assert_eq!(
            restored.request.enhancement_policy,
            EnhancementPolicy::MaxPotential
        );
        assert_eq!(
            restored.request.medium_target_error,
            default_medium_target_error()
        );
        assert_eq!(
            restored.request.finalist_count,
            default_legacy_finalist_count()
        );
        assert!(
            restored.request.variants[0]
                .upgrade
                .owned_item_key
                .is_empty()
        );
    }
}

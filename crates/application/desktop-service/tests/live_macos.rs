#![cfg(target_os = "macos")]

use std::{collections::BTreeMap, env, fs, path::PathBuf};

use simc_adapter::{sha256_file, validate_macos_binary};
use simshredder_desktop_service::{
    CpuChoice, DesktopService, QuickSimRequest, SourceFormat, TopGearRequest,
};
use simshredder_job_runner::{CancellationToken, DispatchResult};
use simshredder_runtime_manager::{RuntimeDoctor, RuntimeRecord};
use simshredder_top_gear::{ChangeKind, UpgradeAction};

fn required_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must name an existing path"))
}

#[test]
#[ignore = "executes the full desktop service flow with an official SimulationCraft runtime"]
fn import_preview_queue_and_result_match_the_gui_request() {
    let executable = required_path("SIMSHREDDER_SIMC");
    let revision =
        env::var("SIMSHREDDER_SIMC_REVISION").expect("SIMSHREDDER_SIMC_REVISION must be provided");
    let identity = validate_macos_binary(&executable).expect("runtime must satisfy the contract");
    let runtime = RuntimeDoctor {
        record: RuntimeRecord {
            id: format!("{}-{revision}", identity.simc_version),
            simc_version: identity.simc_version.clone(),
            build: revision,
            game_version: identity.game_version.clone(),
            channel: identity.channel.clone(),
            executable_sha256: sha256_file(&executable).expect("runtime hash must be available"),
            installed_at_unix_seconds: 1,
        },
        executable,
        identity,
        healthy: true,
    };
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../test-data/fixtures/profiles/file-warrior.simc"),
    )
    .expect("profile fixture must be readable");
    let request = QuickSimRequest {
        source,
        format: SourceFormat::SimcFile,
        iterations: 100,
        fixed_time: false,
        max_time_seconds: 30,
        vary_combat_length: 0.0,
        desired_targets: 2,
        fight_style: "Patchwerk".into(),
        cpu_preset: CpuChoice::Balanced,
        analysis: simshredder_desktop_service::AnalysisOptions::default(),
    };
    let temporary = tempfile::tempdir().expect("temporary app data must be available");
    let service = DesktopService::open(temporary.path()).expect("service must open");

    let preview = service.prepare(&request).expect("preview must succeed");
    assert!(preview.generated_input.contains("iterations=100\n"));
    assert!(preview.generated_input.contains("max_time=30\n"));
    assert!(preview.generated_input.contains("desired_targets=2\n"));
    let (job_id, token) = service
        .enqueue(&request, &runtime)
        .expect("job must enqueue");
    assert!(matches!(
        service.run_next(token).expect("job must dispatch"),
        DispatchResult::Executed { job_id: id, .. } if id == job_id
    ));
    let result = service.result(job_id).expect("result must verify and load");
    assert!(result.result.options.iterations >= 100);
    assert_eq!(result.result.options.max_time_seconds, 30.0);
    assert_eq!(result.result.options.desired_targets, 2);
    assert_eq!(result.generated_input, preview.generated_input);
    let export = service
        .export(job_id, &temporary.path().join("exports"))
        .expect("verified result must export");
    assert_eq!(export.file_count, 5);
    assert_eq!(
        fs::read_to_string(export.directory.join("generated.simc")).expect("exported input"),
        preview.generated_input
    );

    let (cached_job, _) = service
        .enqueue(&request, &runtime)
        .expect("identical job must enqueue");
    assert!(matches!(
        service
            .run_next(CancellationToken::default())
            .expect("cache lookup must dispatch"),
        DispatchResult::CacheHit { job_id: id, .. } if id == cached_job
    ));
}

#[test]
#[ignore = "executes staged Top Gear profilesets with an official SimulationCraft runtime"]
fn top_gear_runs_three_adaptive_stages_through_the_persistent_queue() {
    let executable = required_path("SIMSHREDDER_SIMC");
    let revision =
        env::var("SIMSHREDDER_SIMC_REVISION").expect("SIMSHREDDER_SIMC_REVISION must be provided");
    let identity = validate_macos_binary(&executable).expect("runtime must satisfy the contract");
    let runtime = RuntimeDoctor {
        record: RuntimeRecord {
            id: format!("{}-{revision}", identity.simc_version),
            simc_version: identity.simc_version.clone(),
            build: revision,
            game_version: identity.game_version.clone(),
            channel: identity.channel.clone(),
            executable_sha256: sha256_file(&executable).expect("runtime hash must be available"),
            installed_at_unix_seconds: 1,
        },
        executable,
        identity,
        healthy: true,
    };
    let source = "warrior=DesktopE2E\nlevel=90\nrace=orc\nrole=attack\nspec=fury\nload_default_gear=1\nhead=,id=154029\n\n### Gear from Bags\n# Candidate Helm\n# head=,id=154029,bonus_id=100/200\n".to_owned();
    let mut request = TopGearRequest {
        quick: QuickSimRequest {
            source,
            format: SourceFormat::SimcFile,
            iterations: 100,
            fixed_time: true,
            max_time_seconds: 30,
            vary_combat_length: 0.0,
            desired_targets: 1,
            fight_style: "Patchwerk".into(),
            cpu_preset: CpuChoice::Balanced,
            analysis: simshredder_desktop_service::AnalysisOptions::default(),
        },
        variants: Vec::new(),
        talent_loadouts: Vec::new(),
        profile_options: BTreeMap::new(),
        locked_slots: Default::default(),
        minimum_set_pieces: BTreeMap::new(),
        catalyst_charges: 0,
        balances: BTreeMap::from([("crest".into(), 10), ("valor".into(), 10)]),
        reserves: BTreeMap::from([("crest".into(), 2)]),
        currency_confirmed_at_unix_seconds: 1,
        enhancement_policy: simshredder_top_gear::EnhancementPolicy::MaxPotential,
        target_rank_overrides: BTreeMap::new(),
        upgrade_metadata: None,
        upgrade_metadata_confirmed: false,
        rule_revision: "12.1.0-69465-v1".into(),
        game_build: 69465,
        combination_limit: 16,
        low_iterations: 100,
        high_iterations: 200,
        finalist_count: 2,
        low_target_error: 0.01,
        medium_target_error: 0.002,
        high_target_error: 0.0005,
    };
    let temporary = tempfile::tempdir().expect("temporary app data must be available");
    let service = DesktopService::open(temporary.path()).expect("service must open");
    let discovered = service
        .prepare_top_gear(&request)
        .expect("profile candidates must be discovered");
    request.variants = discovered.variants;
    let candidate = request
        .variants
        .iter_mut()
        .find(|variant| variant.changed)
        .expect("bag fixture must contain a candidate");
    candidate
        .simc_options
        .insert("ilevel".into(), "1000".into());
    candidate.actions = vec![UpgradeAction {
        id: "equip-candidate-head".into(),
        label: "Equip candidate head".into(),
        kind: ChangeKind::Equip,
        cost: BTreeMap::new(),
        depends_on: Vec::new(),
        from_rank: None,
        to_rank: None,
        slot: candidate.slot,
        source_item_id: candidate.source_item_id,
        simc_options_patch: BTreeMap::from([("ilevel".into(), "1000".into())]),
    }];
    let preview = service
        .prepare_top_gear(&request)
        .expect("Top Gear preview must succeed");
    assert_eq!(preview.raw_combinations, 2);
    assert_eq!(preview.valid_combinations, 2);
    assert_eq!(preview.execution_count, 6);

    let low = service
        .start_top_gear(&request, &runtime)
        .expect("low-precision stage must enqueue");
    assert!(matches!(
        service.run_next(low.token.expect("low token")).expect("low stage must run"),
        DispatchResult::Executed { job_id, .. } if Some(job_id) == low.job_id
    ));
    let medium = service
        .advance_top_gear(&low.view.id, &runtime)
        .expect("medium-precision stage must enqueue");
    assert!(matches!(
        service.run_next(medium.token.expect("medium token")).expect("medium stage must run"),
        DispatchResult::Executed { job_id, .. } if Some(job_id) == medium.job_id
    ));
    let high = service
        .advance_top_gear(&low.view.id, &runtime)
        .expect("high-precision stage must enqueue");
    assert!(matches!(
        service
            .run_next(high.token.expect("high token"))
            .expect("high stage must run"),
        DispatchResult::Executed { job_id, .. } if Some(job_id) == high.job_id
    ));
    let result = service
        .top_gear_result(&low.view.id)
        .expect("verified ranking must load");
    assert_eq!(result.ranked.len(), 2);
    assert!(
        result
            .ranked
            .iter()
            .any(|entry| entry.loadout.changed_slots == 0)
    );
    assert!(
        result
            .ranked
            .iter()
            .all(|entry| entry.combined_error >= 0.0)
    );
    assert!(result.action_plan.is_empty());
    assert_eq!(
        result.enhancement_policy,
        simshredder_top_gear::EnhancementPolicy::MaxPotential
    );
    let export = service
        .export_top_gear(&low.view.id, &temporary.path().join("exports"))
        .expect("Top Gear artifacts must export atomically");
    assert_eq!(export.file_count, 6);
    assert!(export.directory.join("final.simc").is_file());
    assert!(export.directory.join("plan.json").is_file());

    let canceled = service
        .start_top_gear(&request, &runtime)
        .expect("recovery session must enqueue");
    let canceled_job = canceled.job_id.expect("low stage has a job");
    let canceled_token = canceled.token.expect("low stage has a cancel token");
    service
        .cancel(canceled_job, &canceled_token)
        .expect("queued Top Gear stage must cancel");
    assert_eq!(
        service
            .top_gear_status(&canceled.view.id)
            .expect("canceled session must load")
            .current_job
            .state,
        "canceled"
    );
    drop(service);

    let recovered = DesktopService::open(temporary.path()).expect("service must reopen");
    assert!(
        recovered
            .top_gear_sessions()
            .expect("sessions must recover")
            .iter()
            .any(
                |session| session.id == canceled.view.id && session.current_job.state == "canceled"
            )
    );
    recovered
        .retry(canceled_job)
        .expect("canceled stage must retry");
    assert!(matches!(
        recovered
            .run_next(CancellationToken::default())
            .expect("retried stage must dispatch"),
        DispatchResult::CacheHit { job_id, .. } if job_id == canceled_job
    ));
    assert_eq!(
        recovered
            .top_gear_status(&canceled.view.id)
            .expect("retried session must load")
            .current_job
            .state,
        "succeeded"
    );
}

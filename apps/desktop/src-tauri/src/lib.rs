use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use simc_adapter::RuntimeManifest;
use simshredder_desktop_service::{
    CharacterProfileView, DesktopService, ExportView, JobView, PreparedQuickSim, PreparedTopGear,
    QuickResultView, QuickSimRequest, TopGearRequest, TopGearResultView, TopGearSessionView,
};
use simshredder_icon_cache::{CacheStatus, IconCache, read_validated_raster};
use simshredder_job_runner::CancellationToken;
use simshredder_runtime_manager::{
    RuntimeDoctor, RuntimeManager, RuntimeRecord, TrustedCatalogKey, download_production_catalog,
};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

mod storage_paths;

use storage_paths::{StorageDefaults, StoragePathsRequest, StoragePathsView};

#[derive(Default)]
struct RunnerState {
    tokens: Mutex<HashMap<i64, CancellationToken>>,
    top_gear_pipelines: Mutex<HashSet<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TopGearRecoveryAction {
    None,
    AttachCurrentJob,
    AdvanceCompletedStage,
}

fn top_gear_recovery_action(
    stage: &str,
    job_state: &str,
    pipeline_active: bool,
    job_active: bool,
) -> TopGearRecoveryAction {
    if stage == "complete" || pipeline_active || job_active {
        TopGearRecoveryAction::None
    } else if matches!(job_state, "queued" | "running") {
        TopGearRecoveryAction::AttachCurrentJob
    } else if job_state == "succeeded" {
        TopGearRecoveryAction::AdvanceCompletedStage
    } else {
        TopGearRecoveryAction::None
    }
}

struct StorageState {
    control_root: PathBuf,
    defaults: StorageDefaults,
    settings_lock: Mutex<()>,
}

#[tauri::command]
fn application_contract() -> ApplicationContract {
    ApplicationContract {
        product: "SimShredder",
        platform: supported_platform(),
        minimum_os: minimum_supported_os(),
        game: "World of Warcraft Retail Live",
    }
}

#[cfg(target_os = "macos")]
const fn supported_platform() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(target_os = "windows")]
const fn supported_platform() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(target_os = "macos")]
const fn minimum_supported_os() -> &'static str {
    "macOS 26.0"
}

#[cfg(target_os = "windows")]
const fn minimum_supported_os() -> &'static str {
    "Windows 10/11 21H2 or later (SimulationCraft upstream)"
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationContract {
    product: &'static str,
    platform: &'static str,
    minimum_os: &'static str,
    game: &'static str,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum WowheadReferenceKind {
    Item,
    Spell,
}

fn wowhead_reference_url(kind: WowheadReferenceKind, id: u32) -> Result<String, String> {
    if id == 0 {
        return Err("Wowhead reference ID must be greater than zero".into());
    }
    let entity = match kind {
        WowheadReferenceKind::Item => "item",
        WowheadReferenceKind::Spell => "spell",
    };
    Ok(format!("https://www.wowhead.com/{entity}={id}"))
}

#[tauri::command]
fn open_wowhead_reference(
    app: tauri::AppHandle,
    kind: WowheadReferenceKind,
    id: u32,
) -> Result<(), String> {
    let url = wowhead_reference_url(kind, id)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| format!("could not open the external reference: {error}"))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeView {
    state: &'static str,
    active: Option<RuntimeRecord>,
    active_data_date: Option<String>,
    installed: Vec<RuntimeRecord>,
    available_version: String,
    available_build: String,
    update_available: bool,
    diagnostic: Option<String>,
}

struct CatalogContext {
    roots: Vec<TrustedCatalogKey>,
    now: u64,
    target: (&'static str, &'static str),
}

fn initialize_storage(app: &tauri::AppHandle) -> Result<StorageState, String> {
    #[cfg(feature = "wdio")]
    if let Some(path) = std::env::var_os("SIMSHREDDER_TEST_APP_DATA") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            let defaults = StorageDefaults {
                workspace: path.join("workspace"),
                simulationcraft: path.join("simulationcraft"),
                icons: path.join("icons"),
                exports: path.join("exports"),
            };
            let _ = storage_paths::load(&path, &defaults)?;
            return Ok(StorageState {
                control_root: path,
                defaults,
                settings_lock: Mutex::new(()),
            });
        }
        return Err("SIMSHREDDER_TEST_APP_DATA must be absolute".into());
    }
    #[cfg(target_os = "macos")]
    let legacy_result = app.path().app_data_dir();
    #[cfg(target_os = "windows")]
    let legacy_result = app.path().app_local_data_dir();
    let legacy = legacy_result
        .map_err(|error| format!("application data directory is unavailable: {error}"))?;
    let parent = legacy
        .parent()
        .ok_or_else(|| "application data parent directory is unavailable".to_owned())?;
    let control_root = parent.join("SimShredder");
    migrate_legacy_root(&legacy, &control_root)?;
    let documents = app
        .path()
        .document_dir()
        .map_err(|error| format!("Documents directory is unavailable: {error}"))?;
    let defaults = StorageDefaults {
        workspace: control_root.join("workspace"),
        simulationcraft: control_root.join("simulationcraft"),
        icons: control_root.join("icons"),
        exports: documents.join("SimShredder Exports"),
    };
    let _ = storage_paths::load(&control_root, &defaults)?;
    Ok(StorageState {
        control_root,
        defaults,
        settings_lock: Mutex::new(()),
    })
}

fn migrate_legacy_root(legacy: &Path, control_root: &Path) -> Result<(), String> {
    if control_root.exists() || !legacy.exists() || legacy == control_root {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(legacy)
        .map_err(|error| format!("legacy application data could not be inspected: {error}"))?;
    if !metadata.file_type().is_dir() {
        return Err("legacy application data is not a regular directory".into());
    }
    std::fs::rename(legacy, control_root).map_err(|error| {
        format!("legacy application data could not be moved to SimShredder: {error}")
    })
}

fn configured_paths(app: &tauri::AppHandle) -> Result<StoragePathsView, String> {
    let state = app.state::<StorageState>();
    let _guard = state
        .settings_lock
        .lock()
        .map_err(|_| "storage settings lock was poisoned".to_owned())?;
    storage_paths::load(&state.control_root, &state.defaults)
}

fn catalog_context() -> Result<CatalogContext, String> {
    let roots: Vec<TrustedCatalogKey> = serde_json::from_str(include_str!(
        "../resources/runtime-catalog-trust-roots.json"
    ))
    .map_err(|error| format!("bundled runtime trust roots are invalid: {error}"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?
        .as_secs();
    #[cfg(target_os = "macos")]
    let target = ("macos", "aarch64");
    #[cfg(target_os = "windows")]
    let target = ("windows", "x86_64");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    compile_error!("SimShredder desktop only supports macOS and Windows");

    Ok(CatalogContext { roots, now, target })
}

fn manifest_from_catalog(
    catalog: simshredder_runtime_manager::VerifiedCatalog,
    target: (&str, &str),
) -> Result<RuntimeManifest, String> {
    catalog
        .payload
        .manifests
        .into_iter()
        .find(|manifest| manifest.platform == target.0 && manifest.architecture == target.1)
        .ok_or_else(|| {
            format!(
                "signed runtime catalog has no {} {} asset",
                target.0, target.1
            )
        })
}

fn cached_available_manifest(manager: &RuntimeManager) -> Result<RuntimeManifest, String> {
    let context = catalog_context()?;
    let catalog = manager
        .verify_and_accept_catalog_or_current_for_target(
            include_bytes!("../resources/runtime-catalog.json"),
            &context.roots,
            context.now,
            context.target.0,
            context.target.1,
        )
        .map_err(|error| format!("bundled or cached runtime catalog was rejected: {error}"))?;
    manifest_from_catalog(catalog, context.target)
}

fn refreshed_available_manifest(manager: &RuntimeManager) -> Result<RuntimeManifest, String> {
    let context = catalog_context()?;

    let baseline = manager
        .verify_and_accept_catalog_or_current_for_target(
            include_bytes!("../resources/runtime-catalog.json"),
            &context.roots,
            context.now,
            context.target.0,
            context.target.1,
        )
        .map_err(|error| format!("bundled or cached runtime catalog was rejected: {error}"))?;

    let catalog = match download_production_catalog().and_then(|bytes| {
        manager.verify_and_accept_catalog_for_target(
            &bytes,
            &context.roots,
            context.now,
            context.target.0,
            context.target.1,
        )
    }) {
        Ok(catalog) => catalog,
        Err(_) => baseline,
    };
    manifest_from_catalog(catalog, context.target)
}

fn manager(app: &tauri::AppHandle) -> Result<RuntimeManager, String> {
    RuntimeManager::open(PathBuf::from(configured_paths(app)?.simulationcraft))
        .map_err(|error| error.to_string())
}

fn desktop_service(app: &tauri::AppHandle) -> Result<DesktopService, String> {
    DesktopService::open(PathBuf::from(configured_paths(app)?.workspace))
        .map_err(|error| error.to_string())
}

fn icon_cache(app: &tauri::AppHandle) -> Result<IconCache, String> {
    IconCache::open(PathBuf::from(configured_paths(app)?.icons)).map_err(|error| error.to_string())
}

fn validated_icon_blob_name(path: &str) -> Option<(&str, &'static str)> {
    let name = path.trim_start_matches('/');
    let (digest, extension) = name.split_once('.')?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    let mime = match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return None,
    };
    Some((name, mime))
}

fn read_icon_blob(icon_root: &Path, name: &str) -> Option<Vec<u8>> {
    let (expected_hash, extension) = name.split_once('.')?;
    let path = icon_root.join("blobs").join(name);
    read_validated_raster(&path, expected_hash, extension).ok()
}

#[tauri::command]
fn storage_paths_get(app: tauri::AppHandle) -> Result<StoragePathsView, String> {
    configured_paths(&app)
}

#[tauri::command]
fn storage_paths_save(
    app: tauri::AppHandle,
    request: StoragePathsRequest,
) -> Result<StoragePathsView, String> {
    if !app
        .state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .is_empty()
    {
        return Err("storage locations cannot change while a simulation is running".into());
    }
    let state = app.state::<StorageState>();
    let _guard = state
        .settings_lock
        .lock()
        .map_err(|_| "storage settings lock was poisoned".to_owned())?;
    storage_paths::save(&state.control_root, &state.defaults, request)
}

#[tauri::command]
fn storage_paths_reset(app: tauri::AppHandle) -> Result<StoragePathsView, String> {
    if !app
        .state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .is_empty()
    {
        return Err("storage locations cannot change while a simulation is running".into());
    }
    let state = app.state::<StorageState>();
    let _guard = state
        .settings_lock
        .lock()
        .map_err(|_| "storage settings lock was poisoned".to_owned())?;
    storage_paths::reset(&state.control_root, &state.defaults)
}

#[tauri::command]
async fn icon_cache_status(app: tauri::AppHandle) -> Result<CacheStatus, String> {
    tauri::async_runtime::spawn_blocking(move || icon_cache(&app).map(|cache| cache.status()))
        .await
        .map_err(|error| format!("icon cache status task failed: {error}"))?
}

#[tauri::command]
async fn icon_cache_clear(app: tauri::AppHandle) -> Result<CacheStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = icon_cache(&app)?;
        cache.clear().map_err(|error| error.to_string())?;
        Ok(cache.status())
    })
    .await
    .map_err(|error| format!("icon cache clear task failed: {error}"))?
}

fn spawn_dispatch(
    app: &tauri::AppHandle,
    service: DesktopService,
    job_id: i64,
    token: CancellationToken,
) -> Result<(), String> {
    app.state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .insert(job_id, token.clone());
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = service.run_next(token);
        if let Ok(mut tokens) = app.state::<RunnerState>().tokens.lock() {
            tokens.remove(&job_id);
        }
    });
    Ok(())
}

fn spawn_top_gear_pipeline(
    app: &tauri::AppHandle,
    service: DesktopService,
    session_id: String,
    runtime: RuntimeDoctor,
    job_id: i64,
    token: CancellationToken,
) -> Result<(), String> {
    {
        let runner = app.state::<RunnerState>();
        let mut pipelines = runner
            .top_gear_pipelines
            .lock()
            .map_err(|_| "runner state lock was poisoned".to_owned())?;
        if !pipelines.insert(session_id.clone()) {
            return Ok(());
        }
    }
    app.state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .insert(job_id, token.clone());
    let app = app.clone();
    let pipeline_session_id = session_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut current_job_id = job_id;
        let mut current_token = token;
        loop {
            let _ = service.run_next(current_token);
            if let Ok(mut tokens) = app.state::<RunnerState>().tokens.lock() {
                tokens.remove(&current_job_id);
            }
            let Ok(started) = service.advance_top_gear(&session_id, &runtime) else {
                break;
            };
            let (Some(next_job_id), Some(next_token)) = (started.job_id, started.token) else {
                break;
            };
            if let Ok(mut tokens) = app.state::<RunnerState>().tokens.lock() {
                tokens.insert(next_job_id, next_token.clone());
            }
            current_job_id = next_job_id;
            current_token = next_token;
        }
        if let Ok(mut pipelines) = app.state::<RunnerState>().top_gear_pipelines.lock() {
            pipelines.remove(&pipeline_session_id);
        }
    });
    Ok(())
}

fn simc_data_date(hotfix: Option<&str>) -> Option<String> {
    let date = hotfix?.split('/').next()?;
    let bytes = date.as_bytes();
    (bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit()))
    .then(|| date.to_owned())
}

fn view_from_manager(
    manager: &RuntimeManager,
    manifest: RuntimeManifest,
) -> Result<RuntimeView, String> {
    let state = manager.state().map_err(|error| error.to_string())?;
    let active = state
        .active_id
        .as_deref()
        .and_then(|id| state.runtimes.iter().find(|runtime| runtime.id == id))
        .cloned();
    let available_id = format!("{}-{}", manifest.simc_version, manifest.build);
    let doctor = manager.doctor_active();
    let (status, diagnostic, active_data_date) = match doctor {
        Ok(Some(RuntimeDoctor {
            healthy: true,
            identity,
            ..
        })) => ("ready", None, simc_data_date(identity.hotfix.as_deref())),
        Ok(Some(_)) => ("damaged", Some("runtime health check failed".into()), None),
        Ok(None) => ("missing", None, None),
        Err(error) => ("damaged", Some(error.to_string()), None),
    };
    Ok(RuntimeView {
        state: status,
        update_available: active
            .as_ref()
            .is_some_and(|record| record.id != available_id),
        active,
        active_data_date,
        installed: state.runtimes,
        available_version: manifest.simc_version,
        available_build: manifest.build,
        diagnostic,
    })
}

#[tauri::command]
async fn runtime_status(app: tauri::AppHandle) -> Result<RuntimeView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = manager(&app)?;
        let manifest = cached_available_manifest(&manager)?;
        view_from_manager(&manager, manifest)
    })
    .await
    .map_err(|error| format!("runtime status task failed: {error}"))?
}

#[tauri::command]
async fn runtime_check_updates(app: tauri::AppHandle) -> Result<RuntimeView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = manager(&app)?;
        let manifest = refreshed_available_manifest(&manager)?;
        view_from_manager(&manager, manifest)
    })
    .await
    .map_err(|error| format!("runtime update check task failed: {error}"))?
}

#[tauri::command]
async fn runtime_install_latest(app: tauri::AppHandle) -> Result<RuntimeView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = manager(&app)?;
        let manifest = refreshed_available_manifest(&manager)?;
        manager
            .install_and_activate(&manifest)
            .map_err(|error| error.to_string())?;
        view_from_manager(&manager, manifest)
    })
    .await
    .map_err(|error| format!("runtime installation task failed: {error}"))?
}

#[tauri::command]
async fn runtime_rollback(app: tauri::AppHandle) -> Result<RuntimeView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = manager(&app)?;
        manager.rollback().map_err(|error| error.to_string())?;
        let manifest = cached_available_manifest(&manager)?;
        view_from_manager(&manager, manifest)
    })
    .await
    .map_err(|error| format!("runtime rollback task failed: {error}"))?
}

#[tauri::command]
async fn quick_prepare(
    app: tauri::AppHandle,
    request: QuickSimRequest,
) -> Result<PreparedQuickSim, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .prepare(&request)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("profile preparation task failed: {error}"))?
}

#[tauri::command]
async fn character_profiles(app: tauri::AppHandle) -> Result<Vec<CharacterProfileView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .character_profiles()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("character profile list task failed: {error}"))?
}

#[tauri::command]
async fn character_profile_save_import(
    app: tauri::AppHandle,
    request: QuickSimRequest,
) -> Result<CharacterProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .save_character_profile_import(&request)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("character profile save task failed: {error}"))?
}

#[tauri::command]
async fn character_profile_set_favorite(
    app: tauri::AppHandle,
    profile_id: String,
    favorite: bool,
) -> Result<CharacterProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .set_character_profile_favorite(&profile_id, favorite)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("character profile favorite task failed: {error}"))?
}

#[tauri::command]
async fn character_profile_delete(app: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .delete_character_profile(&profile_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("character profile delete task failed: {error}"))?
}

#[tauri::command]
async fn character_profile_reload_armory(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<CharacterProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .reload_character_profile_from_armory(&profile_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Armory reload task failed: {error}"))?
}

#[tauri::command]
async fn character_profile_restore_previous(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<CharacterProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .restore_previous_character_profile_input(&profile_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("character profile restore task failed: {error}"))?
}

#[tauri::command]
async fn quick_start(app: tauri::AppHandle, request: QuickSimRequest) -> Result<JobView, String> {
    let worker_app = app.clone();
    let (service, job_id, token) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let runtime = manager(&worker_app)?
            .doctor_active()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "SimulationCraft is not installed".to_owned())?;
        service
            .save_character_profile_import(&request)
            .map_err(|error| error.to_string())?;
        let (job_id, token) = service
            .enqueue(&request, &runtime)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((service, job_id, token))
    })
    .await
    .map_err(|error| format!("Quick Sim enqueue task failed: {error}"))??;
    spawn_dispatch(&app, service.clone(), job_id, token)?;
    service.job(job_id).map_err(|error| error.to_string())
}

#[tauri::command]
async fn quick_job_status(app: tauri::AppHandle, job_id: i64) -> Result<JobView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .job(job_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("job status task failed: {error}"))?
}

#[tauri::command]
async fn quick_cancel(app: tauri::AppHandle, job_id: i64) -> Result<JobView, String> {
    let token = app
        .state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .get(&job_id)
        .cloned()
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&app)?;
        service
            .cancel(job_id, &token)
            .map_err(|error| error.to_string())?;
        service.job(job_id).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("job cancellation task failed: {error}"))?
}

#[tauri::command]
async fn quick_retry(app: tauri::AppHandle, job_id: i64) -> Result<JobView, String> {
    let worker_app = app.clone();
    let service = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        service.retry(job_id).map_err(|error| error.to_string())?;
        Ok::<_, String>(service)
    })
    .await
    .map_err(|error| format!("job retry task failed: {error}"))??;
    spawn_dispatch(&app, service.clone(), job_id, CancellationToken::default())?;
    service.job(job_id).map_err(|error| error.to_string())
}

#[tauri::command]
async fn quick_result(app: tauri::AppHandle, job_id: i64) -> Result<QuickResultView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .result(job_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("result loading task failed: {error}"))?
}

#[tauri::command]
async fn quick_export(app: tauri::AppHandle, job_id: i64) -> Result<ExportView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let destination = PathBuf::from(configured_paths(&app)?.exports);
        desktop_service(&app)?
            .export(job_id, &destination)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("result export task failed: {error}"))?
}

#[tauri::command]
async fn quick_recover(app: tauri::AppHandle) -> Result<Vec<i64>, String> {
    let worker_app = app.clone();
    let (service, jobs) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let jobs = service.recover().map_err(|error| error.to_string())?;
        Ok::<_, String>((service, jobs))
    })
    .await
    .map_err(|error| format!("job recovery task failed: {error}"))??;
    for job_id in &jobs {
        spawn_dispatch(&app, service.clone(), *job_id, CancellationToken::default())?;
    }
    Ok(jobs)
}

#[tauri::command]
async fn quick_jobs(app: tauri::AppHandle) -> Result<Vec<JobView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .recent_jobs()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("job history task failed: {error}"))?
}

#[tauri::command]
async fn quick_delete(app: tauri::AppHandle, job_id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .delete_job(job_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("job deletion task failed: {error}"))?
}

#[tauri::command]
async fn top_gear_prepare(
    app: tauri::AppHandle,
    request: TopGearRequest,
) -> Result<PreparedTopGear, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .prepare_top_gear(&request)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Top Gear preparation task failed: {error}"))?
}

#[tauri::command]
async fn top_gear_start(
    app: tauri::AppHandle,
    request: TopGearRequest,
) -> Result<TopGearSessionView, String> {
    let worker_app = app.clone();
    let (service, runtime, started) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let runtime = manager(&worker_app)?
            .doctor_active()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "SimulationCraft is not installed".to_owned())?;
        let started = service
            .start_top_gear(&request, &runtime)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((service, runtime, started))
    })
    .await
    .map_err(|error| format!("Top Gear enqueue task failed: {error}"))??;
    if let (Some(job_id), Some(token)) = (started.job_id, started.token) {
        spawn_top_gear_pipeline(
            &app,
            service,
            started.view.id.clone(),
            runtime,
            job_id,
            token,
        )?;
    }
    Ok(started.view)
}

#[tauri::command]
async fn top_gear_status(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<TopGearSessionView, String> {
    let worker_app = app.clone();
    let recovery_session_id = session_id.clone();
    let (service, view, recovery) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let view = service
            .top_gear_status(&recovery_session_id)
            .map_err(|error| error.to_string())?;
        let active = worker_app
            .state::<RunnerState>()
            .top_gear_pipelines
            .lock()
            .map_err(|_| "runner state lock was poisoned".to_owned())?
            .contains(&recovery_session_id);
        let recovery =
            if top_gear_recovery_action(&view.stage, &view.current_job.state, active, false)
                == TopGearRecoveryAction::AdvanceCompletedStage
            {
                manager(&worker_app)?
                    .doctor_active()
                    .map_err(|error| error.to_string())?
                    .map(|runtime| {
                        service
                            .advance_top_gear(&recovery_session_id, &runtime)
                            .map(|started| (runtime, started))
                            .map_err(|error| error.to_string())
                    })
                    .transpose()?
            } else {
                None
            };
        Ok::<_, String>((service, view, recovery))
    })
    .await
    .map_err(|error| format!("Top Gear status task failed: {error}"))??;
    let Some((runtime, started)) = recovery else {
        return Ok(view);
    };
    if let (Some(job_id), Some(token)) = (started.job_id, started.token) {
        spawn_top_gear_pipeline(&app, service, session_id, runtime, job_id, token)?;
    }
    Ok(started.view)
}

#[tauri::command]
async fn top_gear_advance(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<TopGearSessionView, String> {
    let worker_app = app.clone();
    let (service, runtime, started) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let runtime = manager(&worker_app)?
            .doctor_active()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "SimulationCraft is not installed".to_owned())?;
        let started = service
            .advance_top_gear(&session_id, &runtime)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((service, runtime, started))
    })
    .await
    .map_err(|error| format!("Top Gear advance task failed: {error}"))??;
    if let (Some(job_id), Some(token)) = (started.job_id, started.token) {
        spawn_top_gear_pipeline(
            &app,
            service,
            started.view.id.clone(),
            runtime,
            job_id,
            token,
        )?;
    }
    Ok(started.view)
}

#[tauri::command]
async fn top_gear_cancel(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<TopGearSessionView, String> {
    let view = desktop_service(&app)?
        .top_gear_status(&session_id)
        .map_err(|error| error.to_string())?;
    let job_id = view.current_job.id;
    let token = app
        .state::<RunnerState>()
        .tokens
        .lock()
        .map_err(|_| "runner state lock was poisoned".to_owned())?
        .get(&job_id)
        .cloned()
        .unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&app)?;
        service
            .cancel(job_id, &token)
            .map_err(|error| error.to_string())?;
        service
            .top_gear_status(&session_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Top Gear cancellation task failed: {error}"))?
}

#[tauri::command]
async fn top_gear_retry(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<TopGearSessionView, String> {
    let service = desktop_service(&app)?;
    let view = service
        .top_gear_status(&session_id)
        .map_err(|error| error.to_string())?;
    let runtime = manager(&app)?
        .doctor_active()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "SimulationCraft is not installed".to_owned())?;
    if view.pipeline_failure.is_some() && view.current_job.state == "succeeded" {
        let started = service
            .advance_top_gear(&session_id, &runtime)
            .map_err(|error| error.to_string())?;
        if let (Some(job_id), Some(token)) = (started.job_id, started.token) {
            spawn_top_gear_pipeline(&app, service, session_id, runtime, job_id, token)?;
        }
        return Ok(started.view);
    }
    let job_id = view.current_job.id;
    service.retry(job_id).map_err(|error| error.to_string())?;
    spawn_top_gear_pipeline(
        &app,
        service.clone(),
        session_id.clone(),
        runtime,
        job_id,
        CancellationToken::default(),
    )?;
    service
        .top_gear_status(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn top_gear_result(
    app: tauri::AppHandle,
    session_id: String,
) -> Result<TopGearResultView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .top_gear_result(&session_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Top Gear result task failed: {error}"))?
}

#[tauri::command]
async fn top_gear_export(app: tauri::AppHandle, session_id: String) -> Result<ExportView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let destination = PathBuf::from(configured_paths(&app)?.exports);
        desktop_service(&app)?
            .export_top_gear(&session_id, &destination)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Top Gear export task failed: {error}"))?
}

#[tauri::command]
async fn top_gear_sessions(app: tauri::AppHandle) -> Result<Vec<TopGearSessionView>, String> {
    let worker_app = app.clone();
    let (service, runtime, sessions) = tauri::async_runtime::spawn_blocking(move || {
        let service = desktop_service(&worker_app)?;
        let runtime = manager(&worker_app)?
            .doctor_active()
            .map_err(|error| error.to_string())?;
        let sessions = service
            .top_gear_sessions()
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((service, runtime, sessions))
    })
    .await
    .map_err(|error| format!("Top Gear session recovery failed: {error}"))??;
    let Some(runtime) = runtime else {
        return Ok(sessions);
    };
    let mut recovered = Vec::with_capacity(sessions.len());
    for session in sessions {
        let pipeline_active = app
            .state::<RunnerState>()
            .top_gear_pipelines
            .lock()
            .map_err(|_| "runner state lock was poisoned".to_owned())?
            .contains(&session.id);
        let job_active = app
            .state::<RunnerState>()
            .tokens
            .lock()
            .map_err(|_| "runner state lock was poisoned".to_owned())?
            .contains_key(&session.current_job.id);
        match top_gear_recovery_action(
            &session.stage,
            &session.current_job.state,
            pipeline_active,
            job_active,
        ) {
            TopGearRecoveryAction::None => {
                recovered.push(session);
                continue;
            }
            TopGearRecoveryAction::AttachCurrentJob => {
                spawn_top_gear_pipeline(
                    &app,
                    service.clone(),
                    session.id.clone(),
                    runtime.clone(),
                    session.current_job.id,
                    CancellationToken::default(),
                )?;
                recovered.push(session);
                continue;
            }
            TopGearRecoveryAction::AdvanceCompletedStage => {}
        }
        let started = service
            .advance_top_gear(&session.id, &runtime)
            .map_err(|error| error.to_string())?;
        if let (Some(job_id), Some(token)) = (started.job_id, started.token) {
            spawn_top_gear_pipeline(
                &app,
                service.clone(),
                started.view.id.clone(),
                runtime.clone(),
                job_id,
                token,
            )?;
        }
        recovered.push(started.view);
    }
    Ok(recovered)
}

#[tauri::command]
async fn top_gear_delete(app: tauri::AppHandle, session_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        desktop_service(&app)?
            .delete_top_gear_session(&session_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Top Gear deletion task failed: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_uri_scheme_protocol("simshredder-icon", |context, request| {
            let response =
                validated_icon_blob_name(request.uri().path()).and_then(|(name, mime)| {
                    let root = PathBuf::from(configured_paths(context.app_handle()).ok()?.icons);
                    read_icon_blob(&root, name).map(|body| (mime, body))
                });
            match response {
                Some((mime, body)) => tauri::http::Response::builder()
                    .status(200)
                    .header(tauri::http::header::CONTENT_TYPE, mime)
                    .header(
                        tauri::http::header::CACHE_CONTROL,
                        "private, max-age=31536000, immutable",
                    )
                    .body(body)
                    .expect("static icon response is valid"),
                None => tauri::http::Response::builder()
                    .status(404)
                    .header(
                        tauri::http::header::CONTENT_TYPE,
                        "text/plain; charset=utf-8",
                    )
                    .body(b"icon not found".to_vec())
                    .expect("static error response is valid"),
            }
        });
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .manage(RunnerState::default())
        .setup(|app| {
            let storage = initialize_storage(app.handle())
                .map_err(|error| std::io::Error::other(format!("storage setup failed: {error}")))?;
            app.manage(storage);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            application_contract,
            storage_paths_get,
            storage_paths_save,
            storage_paths_reset,
            runtime_status,
            runtime_check_updates,
            runtime_install_latest,
            runtime_rollback,
            icon_cache_status,
            icon_cache_clear,
            open_wowhead_reference,
            character_profiles,
            character_profile_save_import,
            character_profile_set_favorite,
            character_profile_delete,
            character_profile_reload_armory,
            character_profile_restore_previous,
            quick_prepare,
            quick_start,
            quick_job_status,
            quick_cancel,
            quick_retry,
            quick_result,
            quick_export,
            quick_recover,
            quick_jobs,
            quick_delete,
            top_gear_prepare,
            top_gear_start,
            top_gear_status,
            top_gear_advance,
            top_gear_cancel,
            top_gear_retry,
            top_gear_result,
            top_gear_export,
            top_gear_sessions,
            top_gear_delete
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SimShredder");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn application_contract_is_retail_live_on_supported_macos() {
        let contract = application_contract();
        assert_eq!(contract.platform, "aarch64-apple-darwin");
        assert_eq!(contract.minimum_os, "macOS 26.0");
        assert!(contract.game.contains("Retail Live"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn application_contract_is_retail_live_on_supported_windows() {
        let contract = application_contract();
        assert_eq!(contract.platform, "x86_64-pc-windows-msvc");
        assert_eq!(
            contract.minimum_os,
            "Windows 10/11 21H2 or later (SimulationCraft upstream)"
        );
        assert!(contract.game.contains("Retail Live"));
    }

    #[test]
    fn icon_protocol_accepts_only_content_addressed_raster_names() {
        let digest = "a".repeat(64);
        assert_eq!(
            validated_icon_blob_name(&format!("/{digest}.png")).map(|value| value.1),
            Some("image/png")
        );
        assert!(validated_icon_blob_name("/../../profile.simc").is_none());
        assert!(validated_icon_blob_name(&format!("/{digest}.svg")).is_none());
        assert!(validated_icon_blob_name(&format!("/{}.png", "A".repeat(64))).is_none());
    }

    #[test]
    fn wowhead_reference_is_exact_and_contains_no_tracking_data() {
        assert_eq!(
            wowhead_reference_url(WowheadReferenceKind::Item, 154029).unwrap(),
            "https://www.wowhead.com/item=154029"
        );
        assert_eq!(
            wowhead_reference_url(WowheadReferenceKind::Spell, 184367).unwrap(),
            "https://www.wowhead.com/spell=184367"
        );
        assert!(wowhead_reference_url(WowheadReferenceKind::Item, 0).is_err());
    }

    #[test]
    fn simc_hotfix_exposes_only_an_iso_game_data_date() {
        assert_eq!(
            simc_data_date(Some("2026-08-25/69497")).as_deref(),
            Some("2026-08-25")
        );
        assert_eq!(simc_data_date(None), None);
        assert_eq!(simc_data_date(Some("August 25/69497")), None);
    }

    #[test]
    fn top_gear_crash_recovery_resumes_without_duplicate_dispatch() {
        assert_eq!(
            top_gear_recovery_action("medium_precision", "queued", false, false),
            TopGearRecoveryAction::AttachCurrentJob
        );
        assert_eq!(
            top_gear_recovery_action("medium_precision", "succeeded", false, false),
            TopGearRecoveryAction::AdvanceCompletedStage
        );
        assert_eq!(
            top_gear_recovery_action("high_precision", "running", true, false),
            TopGearRecoveryAction::None
        );
        assert_eq!(
            top_gear_recovery_action("low_precision", "queued", false, true),
            TopGearRecoveryAction::None
        );
        assert_eq!(
            top_gear_recovery_action("complete", "succeeded", false, false),
            TopGearRecoveryAction::None
        );
        assert_eq!(
            top_gear_recovery_action("low_precision", "failed", false, false),
            TopGearRecoveryAction::None
        );
    }

    #[cfg(unix)]
    #[test]
    fn icon_protocol_does_not_follow_a_symlinked_blob() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let blobs = temporary.path().join("icons/blobs");
        std::fs::create_dir_all(&blobs).unwrap();
        let outside = temporary.path().join("private.txt");
        std::fs::write(&outside, b"not an icon").unwrap();
        let name = format!("{}.png", "a".repeat(64));
        symlink(&outside, blobs.join(&name)).unwrap();

        assert!(read_icon_blob(&temporary.path().join("icons"), &name).is_none());
        assert_eq!(std::fs::read(outside).unwrap(), b"not an icon");
    }

    #[test]
    fn legacy_identifier_directory_moves_to_product_directory_once() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy = temporary.path().join("dev.simshredder.desktop");
        let current = temporary.path().join("SimShredder");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("preserved.txt"), b"history").unwrap();

        migrate_legacy_root(&legacy, &current).unwrap();
        assert!(!legacy.exists());
        assert_eq!(
            std::fs::read(current.join("preserved.txt")).unwrap(),
            b"history"
        );
        migrate_legacy_root(&legacy, &current).unwrap();
    }
}

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "storage-settings.json";
const SETTINGS_BACKUP: &str = ".storage-settings.json.backup";
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageDefaults {
    pub workspace: PathBuf,
    pub simulationcraft: PathBuf,
    pub icons: PathBuf,
    pub exports: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredPaths {
    schema_version: u32,
    workspace: PathBuf,
    simulationcraft: PathBuf,
    icons: PathBuf,
    exports: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePathsRequest {
    pub workspace: String,
    pub simulationcraft: String,
    pub icons: String,
    pub exports: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePathsView {
    pub config_root: String,
    pub workspace: String,
    pub simulationcraft: String,
    pub icons: String,
    pub exports: String,
    pub default_workspace: String,
    pub default_simulationcraft: String,
    pub default_icons: String,
    pub default_exports: String,
}

pub fn load(control_root: &Path, defaults: &StorageDefaults) -> Result<StoragePathsView, String> {
    validate_control_root(control_root)?;
    let settings_path = control_root.join(SETTINGS_FILE);
    recover_interrupted_save(control_root, &settings_path)?;
    let paths = if settings_path.exists() {
        let metadata = fs::symlink_metadata(&settings_path)
            .map_err(|error| format!("storage settings metadata is unavailable: {error}"))?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_SETTINGS_BYTES {
            return Err("storage settings must be a small regular file".into());
        }
        let stored: StoredPaths = serde_json::from_slice(
            &fs::read(&settings_path)
                .map_err(|error| format!("storage settings could not be read: {error}"))?,
        )
        .map_err(|error| format!("storage settings are invalid: {error}"))?;
        if stored.schema_version != 1 {
            return Err(format!(
                "unsupported storage settings schema {}",
                stored.schema_version
            ));
        }
        validate_paths([
            &stored.workspace,
            &stored.simulationcraft,
            &stored.icons,
            &stored.exports,
        ])?;
        stored
    } else {
        StoredPaths {
            schema_version: 1,
            workspace: defaults.workspace.clone(),
            simulationcraft: defaults.simulationcraft.clone(),
            icons: defaults.icons.clone(),
            exports: defaults.exports.clone(),
        }
    };
    Ok(view(control_root, defaults, &paths))
}

pub fn save(
    control_root: &Path,
    defaults: &StorageDefaults,
    request: StoragePathsRequest,
) -> Result<StoragePathsView, String> {
    validate_control_root(control_root)?;
    let stored = StoredPaths {
        schema_version: 1,
        workspace: validated_input("workspace", request.workspace)?,
        simulationcraft: validated_input("SimulationCraft", request.simulationcraft)?,
        icons: validated_input("icon cache", request.icons)?,
        exports: validated_input("exports", request.exports)?,
    };
    validate_paths([
        &stored.workspace,
        &stored.simulationcraft,
        &stored.icons,
        &stored.exports,
    ])?;
    for path in [
        &stored.workspace,
        &stored.simulationcraft,
        &stored.icons,
        &stored.exports,
    ] {
        create_dedicated_directory(path)?;
    }

    let settings_path = control_root.join(SETTINGS_FILE);
    let staging = control_root.join(format!(".{SETTINGS_FILE}.{}.staging", std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(&stored)
        .map_err(|error| format!("storage settings could not be encoded: {error}"))?;
    bytes.push(b'\n');
    fs::write(&staging, bytes)
        .map_err(|error| format!("storage settings could not be staged: {error}"))?;
    protect_file(&staging)?;
    replace_settings(control_root, &settings_path, &staging)?;
    Ok(view(control_root, defaults, &stored))
}

pub fn reset(control_root: &Path, defaults: &StorageDefaults) -> Result<StoragePathsView, String> {
    save(
        control_root,
        defaults,
        StoragePathsRequest {
            workspace: display(&defaults.workspace),
            simulationcraft: display(&defaults.simulationcraft),
            icons: display(&defaults.icons),
            exports: display(&defaults.exports),
        },
    )
}

fn validated_input(label: &str, value: String) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} directory must not be empty"));
    }
    let path = PathBuf::from(trimmed);
    validate_absolute(&path).map_err(|error| format!("{label} directory {error}"))?;
    Ok(path)
}

fn validate_paths(paths: [&PathBuf; 4]) -> Result<(), String> {
    for path in paths {
        validate_absolute(path)?;
        if path.exists() {
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| format!("storage directory metadata is unavailable: {error}"))?;
            if !metadata.file_type().is_dir() {
                return Err(format!("{} is not a regular directory", path.display()));
            }
        }
    }
    for left in 0..paths.len() {
        for right in (left + 1)..paths.len() {
            if paths[left].starts_with(paths[right]) || paths[right].starts_with(paths[left]) {
                return Err(
                    "storage directories must be separate and must not contain one another".into(),
                );
            }
        }
    }
    Ok(())
}

fn validate_absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("storage directories must be absolute and cannot be a filesystem root".into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("storage directories cannot contain '.' or '..' components".into());
    }
    Ok(())
}

fn validate_control_root(control_root: &Path) -> Result<(), String> {
    validate_absolute(control_root)?;
    create_dedicated_directory(control_root)
}

fn create_dedicated_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("directory metadata is unavailable: {error}"))?;
        if !metadata.file_type().is_dir() {
            return Err(format!("{} is not a regular directory", path.display()));
        }
    } else {
        fs::create_dir_all(path)
            .map_err(|error| format!("{} could not be created: {error}", path.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "{} permissions could not be protected: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn protect_file(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            format!("storage settings permissions could not be protected: {error}")
        })?;
    }
    Ok(())
}

fn replace_settings(
    control_root: &Path,
    settings_path: &Path,
    staging: &Path,
) -> Result<(), String> {
    let backup = control_root.join(SETTINGS_BACKUP);
    if backup.exists() {
        let metadata = fs::symlink_metadata(&backup).map_err(|error| {
            format!("stale storage settings backup could not be inspected: {error}")
        })?;
        if !metadata.file_type().is_file() {
            return Err("stale storage settings backup is not a regular file".into());
        }
        fs::remove_file(&backup).map_err(|error| {
            format!("stale storage settings backup could not be removed: {error}")
        })?;
    }
    if settings_path.exists() {
        fs::rename(settings_path, &backup)
            .map_err(|error| format!("current storage settings could not be backed up: {error}"))?;
    }
    if let Err(error) = fs::rename(staging, settings_path) {
        if backup.exists() {
            let _ = fs::rename(&backup, settings_path);
        }
        return Err(format!("storage settings could not be activated: {error}"));
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("storage settings backup could not be removed: {error}"))?;
    }
    Ok(())
}

fn recover_interrupted_save(control_root: &Path, settings_path: &Path) -> Result<(), String> {
    let backup = control_root.join(SETTINGS_BACKUP);
    if !backup.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(&backup)
        .map_err(|error| format!("storage settings backup could not be inspected: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("storage settings backup is not a regular file".into());
    }
    if settings_path.exists() {
        fs::remove_file(&backup).map_err(|error| {
            format!("completed storage settings backup could not be removed: {error}")
        })?;
    } else {
        fs::rename(&backup, settings_path)
            .map_err(|error| format!("storage settings backup could not be recovered: {error}"))?;
    }
    Ok(())
}

fn view(control_root: &Path, defaults: &StorageDefaults, paths: &StoredPaths) -> StoragePathsView {
    StoragePathsView {
        config_root: display(control_root),
        workspace: display(&paths.workspace),
        simulationcraft: display(&paths.simulationcraft),
        icons: display(&paths.icons),
        exports: display(&paths.exports),
        default_workspace: display(&defaults.workspace),
        default_simulationcraft: display(&defaults.simulationcraft),
        default_icons: display(&defaults.icons),
        default_exports: display(&defaults.exports),
    }
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults(root: &Path) -> StorageDefaults {
        StorageDefaults {
            workspace: root.join("SimShredder/workspace"),
            simulationcraft: root.join("SimShredder/simulationcraft"),
            icons: root.join("SimShredder/icons"),
            exports: root.join("Documents/SimShredder Exports"),
        }
    }

    #[test]
    fn defaults_and_custom_paths_round_trip_without_identifier_directory_names() {
        let temporary = tempfile::tempdir().unwrap();
        let defaults = defaults(temporary.path());
        let control = temporary.path().join("SimShredder");
        let initial = load(&control, &defaults).unwrap();
        assert!(initial.config_root.ends_with("SimShredder"));
        assert!(!initial.config_root.contains("dev.simshredder.desktop"));

        let custom = temporary.path().join("custom");
        let saved = save(
            &control,
            &defaults,
            StoragePathsRequest {
                workspace: display(&custom.join("records")),
                simulationcraft: display(&custom.join("simc")),
                icons: display(&custom.join("icons")),
                exports: display(&custom.join("exports")),
            },
        )
        .unwrap();
        assert_eq!(
            load(&control, &defaults).unwrap().workspace,
            saved.workspace
        );
        let reset = reset(&control, &defaults).unwrap();
        assert_eq!(reset.workspace, display(&defaults.workspace));
    }

    #[test]
    fn rejects_relative_overlapping_and_symlinked_directories() {
        let temporary = tempfile::tempdir().unwrap();
        let defaults = defaults(temporary.path());
        let control = temporary.path().join("SimShredder");
        let error = save(
            &control,
            &defaults,
            StoragePathsRequest {
                workspace: "relative".into(),
                simulationcraft: display(&temporary.path().join("simc")),
                icons: display(&temporary.path().join("icons")),
                exports: display(&temporary.path().join("exports")),
            },
        )
        .unwrap_err();
        assert!(error.contains("absolute"));

        let parent = temporary.path().join("parent");
        let error = save(
            &control,
            &defaults,
            StoragePathsRequest {
                workspace: display(&parent),
                simulationcraft: display(&parent.join("simc")),
                icons: display(&temporary.path().join("icons")),
                exports: display(&temporary.path().join("exports")),
            },
        )
        .unwrap_err();
        assert!(error.contains("must not contain"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = temporary.path().join("target");
            fs::create_dir(&target).unwrap();
            let link = temporary.path().join("link");
            symlink(&target, &link).unwrap();
            let error = save(
                &control,
                &defaults,
                StoragePathsRequest {
                    workspace: display(&link),
                    simulationcraft: display(&temporary.path().join("simc")),
                    icons: display(&temporary.path().join("icons")),
                    exports: display(&temporary.path().join("exports")),
                },
            )
            .unwrap_err();
            assert!(error.contains("regular directory"));
        }
    }
}

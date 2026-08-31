use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{Manager, PhysicalPosition, PhysicalSize, Window, WindowEvent};

const STATE_FILE: &str = "window-state.json";
const STATE_BACKUP: &str = ".window-state.json.backup";
const MAX_STATE_BYTES: u64 = 16 * 1024;
const MIN_WIDTH: u32 = 720;
const MIN_HEIGHT: u32 = 560;
const MIN_VISIBLE_EDGE: i64 = 64;
const MAX_DIMENSION: u32 = 32_768;
const DEFAULT_WIDTH: f64 = 1280.0;
const DEFAULT_HEIGHT: f64 = 800.0;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
struct StoredWindowState {
    schema_version: u32,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
}

impl StoredWindowState {
    fn is_valid(self) -> bool {
        self.schema_version == 1
            && (MIN_WIDTH..=MAX_DIMENSION).contains(&self.width)
            && (MIN_HEIGHT..=MAX_DIMENSION).contains(&self.height)
    }
}

pub struct WindowStateTracker {
    path: PathBuf,
    state: Mutex<StoredWindowState>,
}

fn intersects(
    state: StoredWindowState,
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
) -> bool {
    let state_right = i64::from(state.x) + i64::from(state.width);
    let state_bottom = i64::from(state.y) + i64::from(state.height);
    let monitor_right = i64::from(monitor_position.x) + i64::from(monitor_size.width);
    let monitor_bottom = i64::from(monitor_position.y) + i64::from(monitor_size.height);
    let overlap_width =
        state_right.min(monitor_right) - i64::from(state.x).max(i64::from(monitor_position.x));
    let overlap_height =
        state_bottom.min(monitor_bottom) - i64::from(state.y).max(i64::from(monitor_position.y));
    overlap_width >= MIN_VISIBLE_EDGE && overlap_height >= MIN_VISIBLE_EDGE
}

fn read_state(path: &Path) -> Option<StoredWindowState> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > MAX_STATE_BYTES {
        return None;
    }
    let state: StoredWindowState = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    state.is_valid().then_some(state)
}

fn protect_file(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("window state permissions could not be protected: {error}"))?;
    }
    Ok(())
}

fn write_state(path: &Path, state: StoredWindowState) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "window state path has no parent".to_owned())?;
    let staging = parent.join(format!(".{STATE_FILE}.{}.staging", std::process::id()));
    if staging.exists() {
        let metadata = fs::symlink_metadata(&staging)
            .map_err(|error| format!("stale window state could not be inspected: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err("stale window state is not a regular file".into());
        }
        fs::remove_file(&staging)
            .map_err(|error| format!("stale window state could not be removed: {error}"))?;
    }
    let mut bytes = serde_json::to_vec_pretty(&state)
        .map_err(|error| format!("window state could not be encoded: {error}"))?;
    bytes.push(b'\n');
    fs::write(&staging, bytes)
        .map_err(|error| format!("window state could not be staged: {error}"))?;
    protect_file(&staging)?;
    let backup = parent.join(STATE_BACKUP);
    if backup.exists() {
        let metadata = fs::symlink_metadata(&backup)
            .map_err(|error| format!("window state backup could not be inspected: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err("window state backup is not a regular file".into());
        }
        fs::remove_file(&backup)
            .map_err(|error| format!("window state backup could not be removed: {error}"))?;
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("window state could not be inspected: {error}"))?;
        if !metadata.file_type().is_file() {
            return Err("window state is not a regular file".into());
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("window state could not be backed up: {error}"))?;
    }
    if let Err(error) = fs::rename(&staging, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("window state could not be activated: {error}"));
    }
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("window state backup could not be removed: {error}"))?;
    }
    Ok(())
}

fn current_normal_state(window: &Window, previous: StoredWindowState) -> StoredWindowState {
    let maximized = window.is_maximized().unwrap_or(previous.maximized);
    let minimized = window.is_minimized().unwrap_or(false);
    let fullscreen = window.is_fullscreen().unwrap_or(false);
    if maximized || minimized || fullscreen {
        return StoredWindowState {
            maximized,
            ..previous
        };
    }
    let Ok(size) = window.outer_size() else {
        return previous;
    };
    let Ok(position) = window.outer_position() else {
        return previous;
    };
    let next = StoredWindowState {
        schema_version: 1,
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized: false,
    };
    if next.is_valid() { next } else { previous }
}

pub fn initialize(app: &tauri::AppHandle, control_root: &Path) -> Result<(), String> {
    let webview_window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_owned())?;
    let window = webview_window.as_ref().window();
    let path = control_root.join(STATE_FILE);
    let saved = read_state(&path);
    let initial = saved.unwrap_or_else(|| {
        current_normal_state(
            &window,
            StoredWindowState {
                schema_version: 1,
                width: 1280,
                height: 800,
                x: 0,
                y: 0,
                maximized: false,
            },
        )
    });
    app.manage(WindowStateTracker {
        path,
        state: Mutex::new(initial),
    });
    if let Some(state) = saved {
        window
            .set_size(PhysicalSize::new(state.width, state.height))
            .map_err(|error| format!("window size could not be restored: {error}"))?;
        let on_screen = window
            .available_monitors()
            .map_err(|error| format!("display layout could not be read: {error}"))?
            .iter()
            .any(|monitor| intersects(state, *monitor.position(), *monitor.size()));
        if on_screen {
            window
                .set_position(PhysicalPosition::new(state.x, state.y))
                .map_err(|error| format!("window position could not be restored: {error}"))?;
        } else {
            window
                .center()
                .map_err(|error| format!("window could not be centered: {error}"))?;
        }
        if state.maximized {
            window
                .maximize()
                .map_err(|error| format!("window could not be maximized: {error}"))?;
        }
    }
    Ok(())
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    let tracker = window.state::<WindowStateTracker>();
    let Ok(previous) = tracker.state.lock().map(|state| *state) else {
        return;
    };
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let next = current_normal_state(window, previous);
            if let Ok(mut state) = tracker.state.lock() {
                *state = next;
            }
        }
        WindowEvent::CloseRequested { .. } => {
            let next = current_normal_state(window, previous);
            if let Ok(mut state) = tracker.state.lock() {
                *state = next;
            }
            let _ = write_state(&tracker.path, next);
        }
        _ => {}
    }
}

#[tauri::command]
pub fn window_state_reset(
    window: Window,
    tracker: tauri::State<'_, WindowStateTracker>,
) -> Result<(), String> {
    window
        .set_fullscreen(false)
        .map_err(|error| format!("window fullscreen state could not be reset: {error}"))?;
    window
        .unmaximize()
        .map_err(|error| format!("window maximized state could not be reset: {error}"))?;
    window
        .set_size(tauri::LogicalSize::new(DEFAULT_WIDTH, DEFAULT_HEIGHT))
        .map_err(|error| format!("window size could not be reset: {error}"))?;
    window
        .center()
        .map_err(|error| format!("window could not be centered: {error}"))?;
    let fallback = StoredWindowState {
        schema_version: 1,
        width: 1280,
        height: 800,
        x: 0,
        y: 0,
        maximized: false,
    };
    let state = current_normal_state(&window, fallback);
    *tracker
        .state
        .lock()
        .map_err(|_| "window state lock was poisoned".to_owned())? = state;
    write_state(&tracker.path, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(x: i32, y: i32, width: u32, height: u32) -> StoredWindowState {
        StoredWindowState {
            schema_version: 1,
            width,
            height,
            x,
            y,
            maximized: false,
        }
    }

    #[test]
    fn validates_bounds_and_detects_removed_monitors() {
        assert!(state(100, 100, 1280, 800).is_valid());
        assert!(!state(100, 100, 719, 800).is_valid());
        let position = PhysicalPosition::new(0, 0);
        let size = PhysicalSize::new(1920, 1080);
        assert!(intersects(state(100, 100, 1280, 800), position, size));
        assert!(!intersects(state(2500, 100, 1280, 800), position, size));
        assert!(intersects(state(-1000, 100, 1280, 800), position, size));
        assert!(!intersects(state(1900, 1060, 1280, 800), position, size));
    }

    #[test]
    fn state_round_trips_in_the_simshredder_control_root() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join(STATE_FILE);
        let expected = state(-1200, 80, 1440, 900);
        write_state(&path, expected).unwrap();
        assert_eq!(read_state(&path), Some(expected));
        let updated = state(40, 60, 1280, 800);
        write_state(&path, updated).unwrap();
        assert_eq!(read_state(&path), Some(updated));
        assert!(!temporary.path().join(STATE_BACKUP).exists());
        assert!(!path.to_string_lossy().contains("dev.simshredder.desktop"));
    }
}

// The phone build keeps only the no-op `apply_window_mode` / `restore_window_state`; the desktop
// window bookkeeping below is unused there.
#![cfg_attr(mobile, allow(dead_code, unused_imports))]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};

use crate::errors::AppResult;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            x: 0,
            y: 0,
            maximized: false,
        }
    }
}

/// Starting size (logical pixels) of the compact window shown while licence activation,
/// administrator setup and sign-in are on screen. It can be resized and maximised (a small screen
/// or a large display zoom must never hide a button); the page inside scrolls.
pub const COMPACT_WIDTH: f64 = 560.0;
pub const COMPACT_HEIGHT: f64 = 640.0;
/// Smallest size the compact window can be dragged to.
const COMPACT_MIN_WIDTH: f64 = 480.0;
const COMPACT_MIN_HEIGHT: f64 = 420.0;
/// Minimum size of the main window (the same as `tauri.conf.json`).
const MAIN_MIN_WIDTH: f64 = 1024.0;
const MAIN_MIN_HEIGHT: f64 = 600.0;

/// True while the compact window is shown: its size and position are never saved as the main
/// window's remembered state.
static COMPACT_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Milliseconds since the epoch of the last switch; resize/move events right after a switch are
/// the switch's own echo and are not saved either.
static LAST_SWITCH_MS: AtomicU64 = AtomicU64::new(0);
const SWITCH_SETTLE_MS: u64 = 600;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Whether resize/move events should currently be remembered as the main window's state.
fn persisting_state() -> bool {
    !COMPACT_ACTIVE.load(Ordering::SeqCst)
        && now_ms().saturating_sub(LAST_SWITCH_MS.load(Ordering::SeqCst)) > SWITCH_SETTLE_MS
}

/// Window shape requested by the front end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// Centred installer-style window (activation, administrator setup, sign-in): starts compact
    /// but can be resized and maximised.
    Compact,
    /// The normal main window at its remembered size.
    Main,
}

fn window_err(e: &tauri::Error) -> crate::errors::AppError {
    crate::errors::AppError::Internal(anyhow::anyhow!("{e}"))
}

/// Phone: one full-screen window, no compact shape to switch to.
#[cfg(mobile)]
pub fn apply_window_mode(_app: &AppHandle, _mode: WindowMode) -> AppResult<()> {
    Ok(())
}

/// Switches the main window between its compact and normal shapes.
#[cfg(desktop)]
pub fn apply_window_mode(app: &AppHandle, mode: WindowMode) -> AppResult<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    match mode {
        WindowMode::Compact => {
            if COMPACT_ACTIVE.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            LAST_SWITCH_MS.store(now_ms(), Ordering::SeqCst);
            // A failed shrink must not leave the window state frozen as "compact".
            let shrink = || -> AppResult<()> {
                // Remember how the main window looked before shrinking it.
                let maximized = window.is_maximized().map_err(|e| window_err(&e))?;
                let mut state = load_state(app);
                state.maximized = maximized;
                if !maximized {
                    if let Ok(size) = window.inner_size() {
                        if size.width > 0 && size.height > 0 {
                            state.width = size.width;
                            state.height = size.height;
                        }
                    }
                    if let Ok(pos) = window.outer_position() {
                        if pos.x > -30000 && pos.y > -30000 {
                            state.x = pos.x;
                            state.y = pos.y;
                        }
                    }
                }
                save_state(app, &state);

                let compact = tauri::Size::Logical(tauri::LogicalSize {
                    width: COMPACT_WIDTH,
                    height: COMPACT_HEIGHT,
                });
                if maximized {
                    window.unmaximize().map_err(|e| window_err(&e))?;
                }
                // The minimum is replaced first so the shrink is allowed; the window stays
                // resizable and maximisable so no button is ever out of reach.
                window
                    .set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize {
                        width: COMPACT_MIN_WIDTH,
                        height: COMPACT_MIN_HEIGHT,
                    })))
                    .map_err(|e| window_err(&e))?;
                window.set_max_size(None::<tauri::Size>).map_err(|e| window_err(&e))?;
                window.set_size(compact).map_err(|e| window_err(&e))?;
                window.set_resizable(true).map_err(|e| window_err(&e))?;
                window.set_maximizable(true).map_err(|e| window_err(&e))?;
                window.center().map_err(|e| window_err(&e))?;
                Ok(())
            };
            let shrunk = shrink();
            if shrunk.is_err() {
                COMPACT_ACTIVE.store(false, Ordering::SeqCst);
            }
            shrunk?;
        }
        WindowMode::Main => {
            if !COMPACT_ACTIVE.load(Ordering::SeqCst) {
                return Ok(());
            }
            LAST_SWITCH_MS.store(now_ms(), Ordering::SeqCst);
            let mut state = load_state(app);
            if state.width == 0 || state.height == 0 || state.x <= -30000 || state.y <= -30000 {
                state = WindowState::default();
            }
            // The compact window may have been maximised: the remembered size only applies to a
            // window in its normal state.
            if window.is_maximized().unwrap_or(false) {
                if let Err(e) = window.unmaximize() {
                    warn!("Cannot unmaximize the compact window: {e}");
                }
            }
            window.set_resizable(true).map_err(|e| window_err(&e))?;
            window.set_maximizable(true).map_err(|e| window_err(&e))?;
            window.set_max_size(None::<tauri::Size>).map_err(|e| window_err(&e))?;
            window
                .set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize {
                    width: MAIN_MIN_WIDTH,
                    height: MAIN_MIN_HEIGHT,
                })))
                .map_err(|e| window_err(&e))?;
            window
                .set_size(tauri::Size::Physical(tauri::PhysicalSize {
                    width: state.width,
                    height: state.height,
                }))
                .map_err(|e| window_err(&e))?;
            if state.x != 0 || state.y != 0 {
                window
                    .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: state.x,
                        y: state.y,
                    }))
                    .map_err(|e| window_err(&e))?;
            } else {
                window.center().map_err(|e| window_err(&e))?;
            }
            if state.maximized {
                window.maximize().map_err(|e| window_err(&e))?;
            }
            COMPACT_ACTIVE.store(false, Ordering::SeqCst);
        }
    }
    Ok(())
}

/// IPC: the pre-login screens ask for the compact window, the signed-in app for the main one.
/// Needs no session (the screens that call it are shown before sign-in) and changes only the
/// window's size and position.
#[tauri::command]
pub fn set_window_mode(app: AppHandle, mode: WindowMode) -> AppResult<()> {
    apply_window_mode(&app, mode)
}

fn state_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
    std::fs::create_dir_all(&dir).map_err(crate::errors::AppError::Io)?;
    Ok(dir.join("window_state.json"))
}

#[allow(clippy::option_if_let_else)]
pub fn load_state(app: &AppHandle) -> WindowState {
    match state_path(app) {
        Ok(path) => {
            if let Ok(text) = std::fs::read_to_string(&path) {
                serde_json::from_str(&text).unwrap_or_default()
            } else {
                WindowState::default()
            }
        }
        Err(e) => {
            warn!("Cannot determine window state path: {e}");
            WindowState::default()
        }
    }
}

pub fn save_state(app: &AppHandle, state: &WindowState) {
    match state_path(app) {
        Ok(path) => {
            if let Ok(json) = serde_json::to_string_pretty(state) {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("Failed to save window state: {e}");
                }
            }
        }
        Err(e) => warn!("Cannot save window state: {e}"),
    }
}

/// Phone: the system sizes the only window; there is no size or position to remember.
#[cfg(mobile)]
pub fn restore_window_state(_app: &mut tauri::App) -> AppResult<()> {
    Ok(())
}

/// Called in the Tauri setup hook. Applies the saved state to the main window
/// and registers event listeners to persist state on resize/move/close.
#[cfg(desktop)]
pub fn restore_window_state(app: &mut tauri::App) -> AppResult<()> {
    let mut state = load_state(&app.handle().clone());
    let handle = app.handle().clone();

    // Sanitize bad saved values (e.g. from a minimized window on Windows)
    if state.width == 0 || state.height == 0 || state.x <= -30000 || state.y <= -30000 {
        warn!(
            "Window state has invalid values (w={}, h={}, x={}, y={}), resetting to defaults",
            state.width, state.height, state.x, state.y
        );
        state = WindowState::default();
    }

    if let Some(window) = app.get_webview_window("main") {
        if state.maximized {
            window
                .maximize()
                .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
        } else {
            window
                .set_size(tauri::Size::Physical(tauri::PhysicalSize {
                    width: state.width,
                    height: state.height,
                }))
                .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;

            // Only apply position if it was explicitly saved (non-zero)
            if state.x != 0 || state.y != 0 {
                window
                    .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: state.x,
                        y: state.y,
                    }))
                    .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("{e}")))?;
            }
        }

        // Listen for move and resize events to persist state
        window.on_window_event(move |event| {
            match event {
                tauri::WindowEvent::Resized(size) => {
                    // Ignore minimized dimensions (Windows reports 0×0 when minimized)
                    if size.width == 0 || size.height == 0 || !persisting_state() {
                        return;
                    }
                    debug!("Window resized: {size:?}");
                    let s = load_state(&handle);
                    // The maximised flag follows the window; a maximised window's size is the
                    // screen's, not the size to come back to when it is restored.
                    let maximized = handle
                        .get_webview_window("main")
                        .and_then(|w| w.is_maximized().ok())
                        .unwrap_or(s.maximized);
                    let (width, height) = if maximized {
                        (s.width, s.height)
                    } else {
                        (size.width, size.height)
                    };
                    save_state(
                        &handle,
                        &WindowState {
                            width,
                            height,
                            maximized,
                            ..s
                        },
                    );
                }
                tauri::WindowEvent::Moved(pos) => {
                    // Ignore minimized position (Windows uses -32000,-32000)
                    if pos.x <= -30000 || pos.y <= -30000 || !persisting_state() {
                        return;
                    }
                    debug!("Window moved: {pos:?}");
                    let s = load_state(&handle);
                    save_state(
                        &handle,
                        &WindowState {
                            x: pos.x,
                            y: pos.y,
                            ..s
                        },
                    );
                }
                // Minimize to tray instead of closing
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    debug!("Close requested: minimizing to tray");
                    api.prevent_close();
                    if let Some(w) = handle.get_webview_window("main") {
                        w.hide().ok();
                    }
                }
                _ => {}
            }
        });
    }

    Ok(())
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    #[test]
    fn window_mode_reads_only_the_two_known_shapes() {
        assert_eq!(
            serde_json::from_str::<WindowMode>("\"compact\"").unwrap(),
            WindowMode::Compact
        );
        assert_eq!(
            serde_json::from_str::<WindowMode>("\"main\"").unwrap(),
            WindowMode::Main
        );
        assert!(serde_json::from_str::<WindowMode>("\"fullscreen\"").is_err());
    }
}

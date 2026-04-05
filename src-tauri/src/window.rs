use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

const MIN_WIDTH: u32 = 640;
const MIN_HEIGHT: u32 = 480;
const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 800;
const MINIMIZED_SENTINEL: i32 = -32000;

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
                sanitize_state(serde_json::from_str(&text).unwrap_or_default())
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

const fn is_minimized_position(x: i32, y: i32) -> bool {
    x <= MINIMIZED_SENTINEL || y <= MINIMIZED_SENTINEL
}

const fn sanitize_state(mut state: WindowState) -> WindowState {
    if state.width < MIN_WIDTH {
        state.width = DEFAULT_WIDTH;
    }
    if state.height < MIN_HEIGHT {
        state.height = DEFAULT_HEIGHT;
    }
    if is_minimized_position(state.x, state.y) {
        state.x = 0;
        state.y = 0;
    }
    state
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

/// Called in the Tauri setup hook. Applies the saved state to the main window
/// and registers event listeners to persist state on resize/move/close.
pub fn restore_window_state(app: &mut tauri::App) -> AppResult<()> {
    let state = load_state(&app.handle().clone());
    let handle = app.handle().clone();

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
                    // Ignore transient minimized/hidden sizes that would make next startup invisible.
                    if size.width < MIN_WIDTH || size.height < MIN_HEIGHT {
                        return;
                    }
                    debug!("Window resized: {size:?}");
                    let s = load_state(&handle);
                    save_state(
                        &handle,
                        &WindowState {
                            width: size.width,
                            height: size.height,
                            maximized: s.maximized,
                            ..s
                        },
                    );
                }
                tauri::WindowEvent::Moved(pos) => {
                    // Ignore minimized sentinel coordinates (e.g. -32000 on Windows).
                    if is_minimized_position(pos.x, pos.y) {
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

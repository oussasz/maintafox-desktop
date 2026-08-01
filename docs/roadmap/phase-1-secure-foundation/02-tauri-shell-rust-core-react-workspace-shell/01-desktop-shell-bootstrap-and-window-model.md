# Sub-phase 02 — File 01
## Desktop Shell Bootstrap and Window Model

**Phase:** 1 — Secure Foundation  
**Sub-phase:** 02 — Tauri Shell, Rust Core, React Workspace Shell  
**Depends on:** Sub-phase 01 (monorepo scaffold, AppError, health_check, coding standards, CI pipeline)  
**Produces:**
- `src-tauri/tauri.conf.json` — complete window and security configuration
- `src-tauri/src/window.rs` — window-state persistence helper
- `src-tauri/src/tray.rs` — system-tray integration module
- `src-tauri/src/startup.rs` — startup sequencing orchestrator
- `src-tauri/src/lib.rs` (updated) — registers tray, window module, startup hook
- `docs/WINDOW_MODEL.md` — supervisor-readable window architecture reference

**PRD alignment:**
- §3 — Layers: React Presentation + Rust Application Core
- §4.2 — Tauri 2.x capabilities-based security model
- §13.2 — Persistent top bar, stable sidebar, status bar always visible
- §14.1 — Cold start < 4 seconds on reference hardware
- §14.2 — Startup integrity checks; recoverable safe state

---

## Sprint S1 — Tauri Window Configuration, Single-Instance Enforcement, and Startup Sequence

### Context

The first thing the Rust core does after `main()` is configuring the Tauri application builder. The window must open at a sensible minimum size, centered, titled correctly, and with the WebView sandboxed tightly. A second launch of the binary must not open a second window — the existing instance must be focused instead. After the webview is ready, a controlled startup sequence (DB integrity → migration check → entitlement cache load → event to frontend) runs before the user ever sees a loading screen.

Everything in this sprint wires the hardware-to-frontend path that every other module depends on. It is not optional polish; it is the foundation for every IPC command, every startup safety check, and every cold-start performance measurement.

---

### AI Agent Prompt — S1

```
You are an expert Rust and Tauri 2.x engineer working on Maintafox Desktop.
The existing monorepo scaffold is in place from Sub-phase 01:
  - src-tauri/src/main.rs          entry point calling lib::run()
  - src-tauri/src/lib.rs           Tauri builder stub
  - src-tauri/src/errors.rs        AppError / AppResult<T>
  - src-tauri/src/commands/mod.rs  contains health_check command
  - src-tauri/src/db/mod.rs        init_db() and run_migrations() stubs

YOUR TASK: Implement the complete Tauri window configuration, single-instance
enforcement, and startup sequence.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Update src-tauri/tauri.conf.json
─────────────────────────────────────────────────────────────────────────
Replace the placeholder configuration with the following exact structure:

{
  "productName": "Maintafox",
  "version": "0.1.0",
  "identifier": "systems.maintafox.desktop",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "pnpm run dev",
    "beforeBuildCommand": "pnpm run build"
  },
  "app": {
    "security": {
      "csp": "default-src 'self'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' data: asset: https://asset.localhost; style-src 'self' 'unsafe-inline'; font-src 'self' data:;"
    },
    "windows": [
      {
        "label": "main",
        "title": "Maintafox",
        "width": 1280,
        "height": 800,
        "minWidth": 1024,
        "minHeight": 600,
        "center": true,
        "resizable": true,
        "fullscreen": false,
        "decorations": true,
        "visible": false,
        "focus": true
      }
    ]
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  },
  "plugins": {
    "shell": { "open": true }
  }
}

Key decisions to record in a comment at the top of the file:
- "visible": false — window starts hidden so the startup sequence can
  complete before the user sees anything; the Rust startup orchestrator
  calls window.show() only after the DB is ready.
- minWidth/minHeight follows PRD §13 workspace model: sidebar + content
  area does not collapse below 1024×600 on workshop PCs.
- CSP: connect-src includes ipc: and http://ipc.localhost which are the
  two origins Tauri 2.x uses for the IPC bridge. No external HTTP origins
  are allowed.

─────────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/capabilities/default.json
─────────────────────────────────────────────────────────────────────────
Tauri 2.x uses fine-grained capabilities. Create the following file:

{
  "$schema": "../node_modules/@tauri-apps/cli/schema/capability.json",
  "identifier": "default",
  "description": "Maintafox default capability set. Keep minimal.",
  "platforms": ["linux", "macOS", "windows"],
  "windows": ["main"],
  "permissions": [
    "core:default",
    "shell:allow-open",
    "dialog:default",
    "fs:default"
  ]
}

No additional permissions are added without a corresponding ADR entry.

─────────────────────────────────────────────────────────────────────────
STEP 3 — Create src-tauri/src/startup.rs
─────────────────────────────────────────────────────────────────────────
This module owns the ordered startup sequence that runs once the WebView
is ready. Every step is fallible; on any fatal error the window title is
updated and the frontend receives a startup_failed event.

```rust
// src-tauri/src/startup.rs
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info};

use crate::errors::AppResult;

/// Events emitted to the frontend during startup.
/// Corresponding TypeScript types live in shared/ipc-types.ts.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StartupEvent {
    DbReady,
    MigrationsComplete { applied: u32 },
    EntitlementCacheLoaded,
    Ready,
    Failed { reason: String },
}

/// Run the ordered startup sequence.
///
/// Called once from the `setup` hook in lib.rs after the window is created.
/// On success, emits StartupEvent::Ready and calls window.show().
/// On failure, emits StartupEvent::Failed and keeps the window showing a
/// minimal error surface.
pub async fn run_startup_sequence(app: AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("main")
        .expect("main window must exist");

    // Phase 1: database integrity and connection
    info!("startup: initialising database");
    match crate::db::init_db().await {
        Ok(conn) => {
            app.manage(conn);
            emit_event(&app, StartupEvent::DbReady);
        }
        Err(e) => {
            let reason = format!("Database initialisation failed: {e}");
            error!("{}", reason);
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 2: schema migrations
    info!("startup: running migrations");
    match crate::db::run_migrations(&app).await {
        Ok(applied) => {
            emit_event(&app, StartupEvent::MigrationsComplete { applied });
        }
        Err(e) => {
            let reason = format!("Migration failed: {e}");
            error!("{}", reason);
            emit_event(&app, StartupEvent::Failed { reason });
            window.show().ok();
            return Err(e);
        }
    }

    // Phase 3: entitlement cache (stub for Phase 4 — always succeeds here)
    info!("startup: loading entitlement cache");
    emit_event(&app, StartupEvent::EntitlementCacheLoaded);

    // All phases passed — show the window
    info!("startup: sequence complete, showing window");
    emit_event(&app, StartupEvent::Ready);
    window.show().map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;

    Ok(())
}

fn emit_event(app: &AppHandle, event: StartupEvent) {
    if let Err(e) = app.emit("startup_event", event) {
        error!("failed to emit startup event: {e}");
    }
}
```

─────────────────────────────────────────────────────────────────────────
STEP 4 — Update src-tauri/src/lib.rs
─────────────────────────────────────────────────────────────────────────
Replace the existing Tauri builder stub with the following. This wires
single-instance enforcement and the startup sequence:

```rust
// src-tauri/src/lib.rs
mod commands;
mod db;
mod errors;
mod startup;
mod tray;
mod window;

pub use errors::{AppError, AppResult};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // Single-instance: focus existing window on second launch
        .plugin(
            tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    w.set_focus().ok();
                    w.show().ok();
                }
            }),
        )
        .setup(|app| {
            // Tray icon (non-fatal if tray is not supported on this platform)
            if let Err(e) = tray::setup_tray(app) {
                tracing::warn!("System tray unavailable: {e}");
            }

            // Window state (restore previous size/position)
            window::restore_window_state(app)?;

            // Launch async startup sequence
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = startup::run_startup_sequence(handle).await {
                    tracing::error!("Startup sequence failed: {e}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
```

─────────────────────────────────────────────────────────────────────────
STEP 5 — Add Cargo.toml dependency: tauri-plugin-single-instance
─────────────────────────────────────────────────────────────────────────
In src-tauri/Cargo.toml add:

[dependencies]
tauri-plugin-single-instance = "2"

Add a corresponding [features] note: single-instance is included in all
build profiles. No feature-flag is needed for desktop-only targets.

─────────────────────────────────────────────────────────────────────────
STEP 6 — Emit StartupEvent TypeScript types in shared/ipc-types.ts
─────────────────────────────────────────────────────────────────────────
Append to the existing shared/ipc-types.ts:

```typescript
// ─── Startup Events ────────────────────────────────────────────────────────

export type StartupStage =
  | "db_ready"
  | "migrations_complete"
  | "entitlement_cache_loaded"
  | "ready"
  | "failed";

export interface StartupEvent {
  stage: StartupStage;
  /** Present only for stage = "migrations_complete" */
  applied?: number;
  /** Present only for stage = "failed" */
  reason?: string;
}
```

─────────────────────────────────────────────────────────────────────────
STEP 7 — Validation
─────────────────────────────────────────────────────────────────────────
Run:
  cd src-tauri && cargo check --all-features
  pnpm run type-check

Both must pass with zero errors. Do not proceed if there are compile errors.
```

---

### Supervisor Verification Checklist — S1

After the agent completes S1, verify the following before marking the sprint done:

**Window behaviour**
- [ ] Launch the dev build (`pnpm tauri dev`). The window must appear centered, 1280×800 by default.  
- [ ] Resize the window to exactly 1023×600. The window must resist going below 1024×600 (minimum enforced).  
- [ ] While the app is running, run the binary again from the terminal. The existing window must receive focus; no second window opens.

**Startup sequence**
- [ ] Open the browser DevTools console (F12 → Console). After launch you must see exactly these events in order: `{stage:"db_ready"}`, `{stage:"migrations_complete", applied:0}`, `{stage:"entitlement_cache_loaded"}`, `{stage:"ready"}`.  
- [ ] Temporarily rename `app.db` to something invalid so `init_db()` fails. The app must still show a window (not crash), the console must show `{stage:"failed", reason:"..."}`, and Rust logs must contain `ERROR startup sequence failed`.  
- [ ] Restore `app.db` filename.

**Security**
- [ ] In DevTools → Network, attempt to fetch an external URL (`fetch("https://example.com")`). It must be blocked by CSP with a console error.  
- [ ] Open `src-tauri/capabilities/default.json` and confirm only the 4 approved permissions are listed: `core:default`, `shell:allow-open`, `dialog:default`, `fs:default`.

**Compile clean**
- [ ] `cargo check --all-features` passes with zero errors.  
- [ ] `pnpm run type-check` passes with zero errors.  
- [ ] `pnpm run lint` passes with zero warnings on modified files.

**Acceptance sign-off:** sign the supervisor feedback issue with label `sprint:S1-phase1-02` before continuing.

---

## Sprint S2 — System Tray Integration and Window State Persistence

### Context

On industrial workshop PCs, the maintenance application often runs all day. Users minimize it to the tray to free screen space during other tasks and restore it with a single click. A deliberate close policy — minimize to tray rather than quit — prevents accidental data loss when the user presses the OS window-close button. Window size and position must survive application restarts so the workspace feels stable across shifts.

This sprint implements both behaviors using lightweight Rust modules that do not add async complexity: the tray is event-driven, and window state is saved to a compact JSON file.

---

### AI Agent Prompt — S2

```
You are an expert Rust and Tauri 2.x engineer continuing work on Maintafox Desktop.
Sprint S1 has been completed and verified:
  - tauri.conf.json is in place (window starts hidden, CSP set)
  - src-tauri/src/startup.rs runs the startup sequence
  - src-tauri/src/lib.rs registers single-instance plugin and setup hooks
  - shared/ipc-types.ts contains StartupEvent types

YOUR TASK: Implement system tray integration and window-state persistence.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/tray.rs
─────────────────────────────────────────────────────────────────────────
The tray icon provides Show/Hide and Quit actions. The application-close
button minimizes to tray by default (configurable later via Settings).

```rust
// src-tauri/src/tray.rs
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};
use tracing::info;

use crate::errors::AppResult;

pub fn setup_tray<R: Runtime>(app: &mut tauri::App<R>) -> AppResult<()> {
    let show_item = MenuItem::with_id(app, "tray_show", "Afficher Maintafox", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
    let hide_item = MenuItem::with_id(app, "tray_hide", "Masquer", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quitter", true, None::<&str>)
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;

    let menu = Menu::with_items(app, &[&show_item, &hide_item, &quit_item])
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;

    TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().cloned().unwrap())
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => {
                if let Some(w) = app.get_webview_window("main") {
                    w.show().ok();
                    w.set_focus().ok();
                }
            }
            "tray_hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide().ok();
                }
            }
            "tray_quit" => {
                info!("Quit requested from tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click on tray icon: toggle visibility
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    match w.is_visible() {
                        Ok(true) => {
                            w.hide().ok();
                        }
                        _ => {
                            w.show().ok();
                            w.set_focus().ok();
                        }
                    }
                }
            }
        })
        .build(app)
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;

    Ok(())
}
```

─────────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/src/window.rs
─────────────────────────────────────────────────────────────────────────
Window state persistence: save size and position on hide/move/resize,
restore on startup. Use the app's data directory so the file is in the
standard per-user config location.

```rust
// src-tauri/src/window.rs
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};
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

fn state_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
    std::fs::create_dir_all(&dir).map_err(crate::errors::AppError::Io)?;
    Ok(dir.join("window_state.json"))
}

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

/// Called in the Tauri setup hook. Applies the saved state to the main window.
pub fn restore_window_state<R: Runtime>(app: &mut tauri::App<R>) -> AppResult<()> {
    let state = load_state(&app.handle().clone());
    let handle = app.handle().clone();

    if let Some(window) = app.get_webview_window("main") {
        if state.maximized {
            window
                .maximize()
                .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
        } else {
            window
                .set_size(tauri::Size::Physical(tauri::PhysicalSize {
                    width: state.width,
                    height: state.height,
                }))
                .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;

            // Only apply position if it was explicitly saved (non-zero)
            if state.x != 0 || state.y != 0 {
                window
                    .set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: state.x,
                        y: state.y,
                    }))
                    .map_err(|e| crate::errors::AppError::Internal(e.to_string()))?;
            }
        }

        // Listen for move and resize events to persist state
        window.on_window_event(move |event| {
            match event {
                tauri::WindowEvent::Resized(size) => {
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
```

─────────────────────────────────────────────────────────────────────────
STEP 3 — Wire minimize-to-tray into CloseRequested handler (lib.rs)
─────────────────────────────────────────────────────────────────────────
The CloseRequested handler in window.rs already prevents close and hides
the window. Verify that lib.rs does NOT call app.exit() on close. The
existing lib.rs from S1 is correct — the window.rs handler handles it.
No change needed to lib.rs for this step; just confirm the wiring.

─────────────────────────────────────────────────────────────────────────
STEP 4 — Write docs/WINDOW_MODEL.md
─────────────────────────────────────────────────────────────────────────
Create this supervisor-facing reference document:

# Maintafox Window Model

## Window Labels
| Label | Purpose |
|-------|---------|
| `main` | Primary workspace — the only window in Phase 1 |

## Startup Lifecycle
1. Rust `lib::run()` → Tauri builder registers plugins and setup hook.
2. `window::restore_window_state()` restores saved size/position.
3. `startup::run_startup_sequence()` runs DB init → migrations → entitlement cache.
4. On success, `window.show()` is called; user sees the app.
5. On failure, `window.show()` is still called; frontend receives `startup_failed` event.

## Window Behavior
- **Minimum size:** 1024 × 600 px (enforced by `tauri.conf.json`).
- **Default size:** 1280 × 800 px on first launch.
- **Single instance:** second binary launch focuses the existing window.
- **Close button:** minimizes to system tray (does NOT quit the process).
- **Quit:** reachable only from tray menu → Quitter, or OS task manager.

## Window State Persistence
- Saved to `{app_data_dir}/window_state.json` on every resize and move event.
- Contents: `{ width, height, x, y, maximized }`.
- Restored in the `setup` hook before the window is shown.

## System Tray Menu (French labels, Phase 1)
| Menu Item | Action |
|-----------|--------|
| Afficher Maintafox | Show + focus the main window |
| Masquer | Hide the main window |
| Quitter | Exit the application (exit code 0) |

## Allowed Capabilities (capabilities/default.json)
- `core:default` — required Tauri core IPC surface
- `shell:allow-open` — opens external URLs in the system browser
- `dialog:default` — file-open and message dialogs
- `fs:default` — read/write within allowed paths only

## Future Windows
Phase 2+ may add:
- `splash` — a minimal splash screen (loaded before `main` is visible)
- `pinned-panel` — a compact floating panel for quick DI creation
Each new window requires a capabilities entry and an IPC_COMMAND_REGISTRY note.

─────────────────────────────────────────────────────────────────────────
STEP 5 — Add tauri-plugin-single-instance to Cargo.toml (if not done in S1)
─────────────────────────────────────────────────────────────────────────
Ensure src-tauri/Cargo.toml contains:
  tauri-plugin-single-instance = "2"

─────────────────────────────────────────────────────────────────────────
STEP 6 — Validation
─────────────────────────────────────────────────────────────────────────
  cd src-tauri && cargo check --all-features
  pnpm run type-check
  pnpm run lint

All must pass with zero errors and zero warnings on modified files.
```

---

### Supervisor Verification Checklist — S2

**Tray behaviour**
- [ ] Launch the app. A Maintafox icon appears in the system tray (Windows taskbar notification area / macOS menu bar).  
- [ ] Right-click the tray icon. The menu shows three French-labelled items: "Afficher Maintafox", "Masquer", "Quitter".  
- [ ] Click "Masquer". The window disappears but the process remains alive (check Task Manager).  
- [ ] Click the tray icon (left-click). The window reappears and receives focus.  
- [ ] Click the OS window-close button (×). The window hides instead of closing; the process stays alive.  
- [ ] Click "Quitter" from tray. The process terminates cleanly.

**Window state persistence**
- [ ] Resize the window to an unusual size (e.g., 1400×900). Close via tray Quitter. Relaunch. The window must open at 1400×900.  
- [ ] Move the window to a non-center position. Quit and relaunch. The window must reopen at the same screen position.  
- [ ] Locate `window_state.json` in the app data directory (`%APPDATA%\systems.maintafox.desktop\` on Windows). Confirm it contains the correct `width`, `height`, `x`, `y` fields.

**Compile clean**
- [ ] `cargo check --all-features` — zero errors.  
- [ ] `pnpm run type-check` — zero errors.

**Acceptance sign-off:** sign the supervisor feedback issue with label `sprint:S2-phase1-02`.

---

## Sprint S3 — Window Integrity Tests and Tauri Configuration Audit

### Context

The window model is the lowest-level runtime contract of the entire application. Before work proceeds into the Rust application core (File 02) and the React layout (File 03), we run a structured audit of the Tauri configuration and write automated tests that verify the key behaviors established in S1 and S2. These tests protect the window model from accidental regression throughout the remaining 100+ files of the roadmap.

---

### AI Agent Prompt — S3

```
You are an expert Tauri 2.x and Rust test engineer continuing work on Maintafox Desktop.
Sprints S1 and S2 are complete and verified. The window model (conf, tray, window.rs,
startup.rs) is in place.

YOUR TASK: Write the window configuration audit and automated tests.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Add tests/window_model_tests.rs (unit tests for window.rs helpers)
─────────────────────────────────────────────────────────────────────────
Create src-tauri/src/window_tests.rs with the following test module that
exercises the serialization and default behavior of WindowState:

```rust
// src-tauri/src/window_tests.rs
// Run with: cargo test -- window_tests
#[cfg(test)]
mod window_tests {
    use crate::window::WindowState;

    #[test]
    fn default_window_state_has_expected_values() {
        let s = WindowState::default();
        assert_eq!(s.width, 1280);
        assert_eq!(s.height, 800);
        assert!(!s.maximized);
    }

    #[test]
    fn window_state_round_trips_json() {
        let original = WindowState {
            width: 1400,
            height: 900,
            x: 100,
            y: 50,
            maximized: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: WindowState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.width, original.width);
        assert_eq!(restored.height, original.height);
        assert_eq!(restored.x, original.x);
        assert_eq!(restored.y, original.y);
        assert_eq!(restored.maximized, original.maximized);
    }

    #[test]
    fn window_state_missing_fields_fall_back_to_defaults() {
        // Simulate a partial JSON from a future version with extra fields
        let partial = r#"{"width":1600,"height":1000,"x":0,"y":0,"maximized":false}"#;
        let state: WindowState = serde_json::from_str(partial).expect("deserialize partial");
        assert_eq!(state.width, 1600);
    }

    #[test]
    fn window_state_corrupt_json_falls_back_cleanly() {
        // Corrupted state file must not panic; the caller uses unwrap_or_default()
        let bad = r#"{"width":"not_a_number"}"#;
        let result = serde_json::from_str::<WindowState>(bad);
        assert!(result.is_err(), "should fail to deserialize");
        // The caller in window.rs does: serde_json::from_str(&text).unwrap_or_default()
        let state: WindowState = serde_json::from_str(bad).unwrap_or_default();
        assert_eq!(state.width, 1280); // falls back to default
    }
}
```

Add the module declaration to src-tauri/src/lib.rs:
  #[cfg(test)]
  mod window_tests;

─────────────────────────────────────────────────────────────────────────
STEP 2 — Add tests/startup_tests.rs (unit tests for StartupEvent serialization)
─────────────────────────────────────────────────────────────────────────
Create src-tauri/src/startup_tests.rs:

```rust
// src-tauri/src/startup_tests.rs
#[cfg(test)]
mod startup_tests {
    use crate::startup::StartupEvent;

    #[test]
    fn db_ready_event_serializes_correctly() {
        let event = StartupEvent::DbReady;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"db_ready""#));
    }

    #[test]
    fn migrations_complete_event_includes_applied_count() {
        let event = StartupEvent::MigrationsComplete { applied: 3 };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"migrations_complete""#));
        assert!(json.contains(r#""applied":3"#));
    }

    #[test]
    fn failed_event_includes_reason() {
        let event = StartupEvent::Failed {
            reason: "DB locked".to_string(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"failed""#));
        assert!(json.contains(r#""reason":"DB locked""#));
    }

    #[test]
    fn ready_event_serializes_as_snake_case_stage() {
        let event = StartupEvent::Ready;
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains(r#""stage":"ready""#));
    }
}
```

Add the module declaration to lib.rs:
  #[cfg(test)]
  mod startup_tests;

─────────────────────────────────────────────────────────────────────────
STEP 3 — tauri.conf.json audit script
─────────────────────────────────────────────────────────────────────────
Create scripts/audit-tauri-conf.ts — a small TypeScript script that
parses tauri.conf.json and asserts the security-critical properties.
Supervisor runs this at any time to verify the config has not drifted.

```typescript
// scripts/audit-tauri-conf.ts
import { readFileSync } from "fs";
import { resolve } from "path";

const confPath = resolve(__dirname, "../src-tauri/tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));

type AuditResult = { pass: boolean; message: string };
const results: AuditResult[] = [];

function check(condition: boolean, message: string): void {
  results.push({ pass: condition, message });
}

// Window constraints
const win = conf?.app?.windows?.[0];
check(win?.visible === false, "window.visible must be false (shows after startup sequence)");
check((win?.minWidth ?? 0) >= 1024, "window.minWidth must be >= 1024");
check((win?.minHeight ?? 0) >= 600, "window.minHeight must be >= 600");
check(win?.label === "main", "window.label must be 'main'");

// CSP
const csp: string = conf?.app?.security?.csp ?? "";
check(csp.includes("default-src 'self'"), "CSP must include default-src 'self'");
check(csp.includes("connect-src 'self' ipc:"), "CSP must allow ipc: for Tauri bridge");
check(!csp.includes("unsafe-eval"), "CSP must NOT include unsafe-eval");

// Version
check(typeof conf?.version === "string", "version must be a string");
check(
  conf?.identifier === "systems.maintafox.desktop",
  "identifier must be 'systems.maintafox.desktop'"
);

// Report
let failures = 0;
for (const r of results) {
  const icon = r.pass ? "✓" : "✗";
  console.log(`  ${icon}  ${r.message}`);
  if (!r.pass) failures++;
}
console.log(`\n${results.length} checks, ${failures} failures`);
process.exit(failures > 0 ? 1 : 0);
```

Add to package.json scripts:
  "audit:tauri-conf": "tsx scripts/audit-tauri-conf.ts"

─────────────────────────────────────────────────────────────────────────
STEP 4 — capabilities audit helper
─────────────────────────────────────────────────────────────────────────
Append to scripts/audit-tauri-conf.ts a second audit block that reads
src-tauri/capabilities/default.json and verifies only the 4 allowed
permissions are present:

```typescript
// Append to scripts/audit-tauri-conf.ts
const capPath = resolve(__dirname, "../src-tauri/capabilities/default.json");
let cap: { permissions?: string[] } = {};
try {
  cap = JSON.parse(readFileSync(capPath, "utf8"));
} catch {
  check(false, "capabilities/default.json must exist and be valid JSON");
}

const ALLOWED_PERMISSIONS = new Set([
  "core:default",
  "shell:allow-open",
  "dialog:default",
  "fs:default",
]);

const actual = new Set(cap.permissions ?? []);
for (const perm of actual) {
  check(ALLOWED_PERMISSIONS.has(perm), `Unexpected capability permission: '${perm}'`);
}
check(actual.size > 0, "capabilities/default.json must define at least one permission");
```

─────────────────────────────────────────────────────────────────────────
STEP 5 — Run everything and confirm green
─────────────────────────────────────────────────────────────────────────
  cd src-tauri && cargo test -- --nocapture
  pnpm run audit:tauri-conf
  pnpm run type-check
  pnpm run lint

All four commands must exit 0.

─────────────────────────────────────────────────────────────────────────
STEP 6 — Update CHANGELOG.md [Unreleased]
─────────────────────────────────────────────────────────────────────────
Append under [Unreleased] → Added in CHANGELOG.md:
- Desktop shell: Tauri window configuration with minimum 1024×600, hidden at startup
- Desktop shell: Single-instance enforcement (second launch focuses existing window)
- Desktop shell: Startup sequence (DB ready → migrations → entitlement cache → ready)
- Desktop shell: System tray with Show/Hide/Quit (French labels)
- Desktop shell: Window state persistence (size, position, maximized state)
- Desktop shell: Minimize-to-tray on OS close-button
- Tests: WindowState serialization, StartupEvent serialization
- Scripts: audit-tauri-conf.ts for CSP, capability, and window constraint verification
- Docs: docs/WINDOW_MODEL.md — window lifecycle and tray reference
```

---

### Supervisor Verification Checklist — S3

**Automated tests**
- [ ] Run `cargo test -- --nocapture` from within `src-tauri/`. All 8 unit tests must pass (4 in `window_tests`, 4 in `startup_tests`).  
- [ ] Run `pnpm run audit:tauri-conf`. All checks must show ✓ and the script must exit 0.  
- [ ] Deliberately break the CSP in `tauri.conf.json` by adding `unsafe-eval`. Run the audit script again — it must exit 1 and show one ✗. Restore the correct CSP.  
- [ ] Deliberately add a fifth permission (`"notification:default"`) to `capabilities/default.json`. Audit script must flag it with ✗. Restore the file.

**CHANGELOG**
- [ ] Open `CHANGELOG.md`. Under `[Unreleased]` → `Added` there must be at least 8 bullet points summarizing Sub-phase 02 File 01 work.

**Full compile**
- [ ] `cargo check --all-features` — zero errors.  
- [ ] `pnpm run type-check` — zero errors.  
- [ ] `pnpm run lint` — zero warnings on modified files.

**Acceptance sign-off:** sign the supervisor feedback issue with label `sprint:S3-phase1-02` before proceeding to File 02.

---

## File 01 — Completion Summary

By the end of this file the following permanent artifacts exist in the repository:

| Artifact | Location | Purpose |
|----------|----------|---------|
| `tauri.conf.json` | `src-tauri/` | Window constraints, CSP, bundle config |
| `capabilities/default.json` | `src-tauri/` | Minimal Tauri 2.x capability set |
| `startup.rs` | `src-tauri/src/` | Ordered startup sequence, emits StartupEvent |
| `tray.rs` | `src-tauri/src/` | System tray with French labels |
| `window.rs` | `src-tauri/src/` | State persistence, minimize-to-tray, close intercept |
| `window_tests.rs` | `src-tauri/src/` | 4 unit tests for WindowState |
| `startup_tests.rs` | `src-tauri/src/` | 4 unit tests for StartupEvent serialization |
| `audit-tauri-conf.ts` | `scripts/` | CI-ready config compliance checker |
| `WINDOW_MODEL.md` | `docs/` | Supervisor-facing architecture reference |
| `shared/ipc-types.ts` | `shared/` (updated) | StartupEvent TypeScript types |

The cold-start path is: `lib::run()` → single-instance guard → tray setup → window state restore → async startup sequence → DB init → migrations → entitlement stub → `window.show()`. This path is the immutable backbone for every future module.

# Maintafox Window Model

This document describes the current Phase 1 desktop window contract implemented in
Maintafox Desktop. It is intended for supervisor review and regression checking.

## Window Labels

| Label | Purpose |
|---|---|
| `main` | Primary workspace window and the only application window in Phase 1 File 01 |

## Startup Lifecycle

1. `lib::run()` builds the Tauri application, registers plugins, and installs the single-instance guard.
2. `tray::setup_tray()` registers the system tray menu and left-click toggle behavior.
3. `window::restore_window_state()` restores the persisted size and position before the window is shown.
4. `startup::run_startup_sequence()` runs asynchronously and emits `startup_event` updates to the frontend.
5. On success, the startup sequence emits `db_ready` -> `migrations_complete` -> `entitlement_cache_loaded` -> `ready`, then calls `window.show()`.
6. On failure, the startup sequence emits `failed`, logs the startup error, and still shows the window so the application fails into a recoverable visible state instead of crashing silently.

## Window Behavior

- **Minimum size:** `1024 x 600` enforced in `src-tauri/tauri.conf.json`
- **Default size:** `1280 x 800` on first launch
- **Initial visibility:** hidden until the startup sequence reaches a terminal state
- **Single instance:** a second launch focuses and reveals the existing `main` window
- **Close button:** the OS close action is intercepted and hides the window instead of terminating the process
- **Quit path:** explicit quit is available from the tray menu via `Quitter`

## Window State Persistence

- Persisted file: `{app_data_dir}/window_state.json`
- Current Windows location: `%APPDATA%\systems.maintafox.desktop\window_state.json`
- Persisted fields: `width`, `height`, `x`, `y`, `maximized`
- Save triggers: window move and resize events
- Restore point: Tauri setup hook before the window is shown to the user

## System Tray Menu

| Menu Item | Action |
|---|---|
| `Afficher Maintafox` | Show and focus the `main` window |
| `Masquer` | Hide the `main` window while leaving the process alive |
| `Quitter` | Exit the application with code `0` |

Left-click on the tray icon toggles window visibility. If the window is hidden, it is shown and focused.

## Allowed Capabilities

The default Phase 1 capability set is defined in `src-tauri/capabilities/default.json` and is intentionally minimal.

- `core:default`
- `shell:allow-open`
- `dialog:default`
- `fs:default`

Any additional capability requires explicit review before being added.

## Supporting Verification Artifacts

- `src-tauri/src/window_tests.rs` verifies `WindowState` defaults and serialization behavior
- `src-tauri/src/startup_tests.rs` verifies `StartupEvent` serialization behavior
- `scripts/audit-tauri-conf.ts` audits window constraints, CSP, application identity, and capability drift

## Future Window Expansion

Phase 2 and later may introduce additional windows such as a splash screen or utility panel. Each new window must:

1. Receive an explicit window label
2. Be added to Tauri capabilities intentionally
3. Be reflected in the IPC and supervisor documentation
4. Preserve the current single-instance and startup-integrity guarantees
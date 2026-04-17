# Sub-phase 02 — File 02
## Rust Core: App Lifecycle and Command Boundary

**Phase:** 1 — Secure Foundation  
**Sub-phase:** 02 — Tauri Shell, Rust Core, React Workspace Shell  
**Depends on:** File 01 (tauri.conf.json, startup.rs, tray.rs, window.rs, lib.rs wiring)  
**Produces:**
- `src-tauri/src/state.rs` — `AppState` struct: db connection, session manager stub, config cache
- `src-tauri/src/background/mod.rs` — background task supervisor (spawn, track, shutdown)
- `src-tauri/src/commands/app.rs` — lifecycle IPC commands: `get_app_info`, `get_startup_state`
- `src-tauri/src/commands/mod.rs` (updated) — registers new app commands
- `src-tauri/src/lib.rs` (updated) — wires `AppState` into Tauri managed state
- `src-tauri/src/startup.rs` (updated) — injects `AppState` components after init
- `docs/IPC_COMMAND_REGISTRY.md` (updated) — adds `get_app_info`, `get_startup_state`
- `shared/ipc-types.ts` (updated) — TypeScript types for new commands

**PRD alignment:**
- §3 — Rust Application Core layer: business orchestration, IPC commands, background jobs, policy enforcement
- §3 — Architectural rule 3: WebView and Rust are separate trust domains joined only through narrow, typed IPC
- §4.2 — Tokio async runtime, tracing for structured diagnostics
- §14.1 — Background execution must not block UI responsiveness
- §14.4 — Structured logs, sync traces, and migration reports collectable

---

## Sprint S1 — AppState: Managed State, DB Connection, and Config Cache

### Context

Every Tauri IPC command that needs database access or application configuration must receive it through dependency injection, not global statics. Tauri's managed state system (`app.manage()` + `tauri::State<T>` in handlers) is the correct mechanism. This sprint defines `AppState`, populates it during the startup sequence, and verifies that all existing IPC commands can access it correctly.

`AppState` in Phase 1 has two concrete components: the database connection pool (from `init_db()`) and a configuration cache (`AppConfig` loaded from the settings table or defaults). The session manager is introduced as a typed stub — it will be fully implemented in Sub-phase 04 (Authentication and RBAC). Adding the stub now allows downstream code to reference `AppState::session` without coupling to a later sprint.

---

### AI Agent Prompt — S1

```
You are an expert Rust and Tauri 2.x engineer continuing work on Maintafox Desktop.
Previous work is in place:
  - src-tauri/src/startup.rs — run_startup_sequence(), emits StartupEvent
  - src-tauri/src/lib.rs    — setup hook calls startup::run_startup_sequence()
  - src-tauri/src/db/mod.rs  — init_db() returns a connection, run_migrations() is stubbed
  - src-tauri/src/errors.rs  — AppError / AppResult<T>

YOUR TASK: Define AppState and wire it into the Tauri managed state system.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/state.rs
─────────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/state.rs
//! Application-wide shared state injected into all Tauri IPC commands.
//!
//! Rules:
//!   - AppState is immutable after initialization (Arc wraps mutable sub-components).
//!   - No global statics — all access is through tauri::State<AppState>.
//!   - Session manager is a stub in Phase 1; Sub-phase 04 replaces the inner type.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Database connection pool managed by sea-orm.
/// Concrete type is sea_orm::DatabaseConnection; SQLite in WAL mode.
pub type DbPool = sea_orm::DatabaseConnection;

/// Application-wide configuration cache.
/// Populated from the `system_config` table on startup; fallback to compiled defaults.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub default_locale: String,
    pub max_offline_grace_hours: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: "Maintafox".to_string(),
            default_locale: "fr".to_string(),
            max_offline_grace_hours: 72,
        }
    }
}

/// Phase 1 session manager stub.
/// Sub-phase 04 replaces this with a real session keying and lifecycle implementation.
#[derive(Debug, Default)]
pub struct SessionManagerStub {
    /// Whether there is an active authenticated session.
    pub has_active_session: bool,
}

/// Central application state shared across all IPC commands.
///
/// Obtain via `tauri::State<AppState>` in command handlers.
/// Components are wrapped in Arc<RwLock<>> only where mutation after init is needed;
/// the db pool and config are read-heavy and use a simpler shared reference model.
#[derive(Debug)]
pub struct AppState {
    /// Live database connection pool. Never clone the pool; always use &self.db.
    pub db: DbPool,
    /// Application configuration cache. Arc<RwLock<>> so Settings module can hot-reload.
    pub config: Arc<RwLock<AppConfig>>,
    /// Session manager stub (Phase 1). Replaced in Sub-phase 04.
    pub session: Arc<RwLock<SessionManagerStub>>,
}

impl AppState {
    /// Construct from a database connection. Config and session start at defaults.
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            config: Arc::new(RwLock::new(AppConfig::default())),
            session: Arc::new(RwLock::new(SessionManagerStub::default())),
        }
    }
}
```

─────────────────────────────────────────────────────────────────────────
STEP 2 — Update src-tauri/src/db/mod.rs to return DbPool
─────────────────────────────────────────────────────────────────────────
The existing `init_db()` stub must return `AppResult<sea_orm::DatabaseConnection>`.
Update it as follows (keep WAL pragma and FK pragma):

```rust
// src-tauri/src/db/mod.rs
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;
use tracing::info;

use crate::errors::AppResult;

pub async fn init_db() -> AppResult<DatabaseConnection> {
    // Resolve path relative to Tauri app data directory at runtime
    // For Phase 1 dev builds, use a local file in the project root
    let db_path = {
        #[cfg(debug_assertions)]
        { "sqlite:./dev.db?mode=rwc".to_string() }
        #[cfg(not(debug_assertions))]
        { "sqlite:./data/app.db?mode=rwc".to_string() }
    };

    info!("Connecting to database: {db_path}");

    let mut opts = ConnectOptions::new(&db_path);
    opts.max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .sqlx_logging(false);

    let db = Database::connect(opts)
        .await
        .map_err(|e| crate::errors::AppError::Database(e.to_string()))?;

    // Enable WAL mode and foreign keys
    use sea_orm::ConnectionTrait;
    db.execute_unprepared("PRAGMA journal_mode=WAL;")
        .await
        .map_err(|e| crate::errors::AppError::Database(e.to_string()))?;
    db.execute_unprepared("PRAGMA foreign_keys=ON;")
        .await
        .map_err(|e| crate::errors::AppError::Database(e.to_string()))?;

    info!("Database connected (WAL mode, FK enforcement enabled)");
    Ok(db)
}

pub async fn run_migrations(_app: &tauri::AppHandle) -> AppResult<u32> {
    // TODO Sub-phase 03: replace with sea-orm-migration runner
    // Returns the number of migrations applied (0 on a fresh schema in Phase 1)
    tracing::info!("run_migrations: stub — no migrations in Phase 1");
    Ok(0)
}
```

─────────────────────────────────────────────────────────────────────────
STEP 3 — Update src-tauri/src/startup.rs to build AppState
─────────────────────────────────────────────────────────────────────────
After `init_db()` succeeds, construct `AppState` and call `app.manage()`:

Replace the `// Phase 1: database integrity` block in startup.rs with:

```rust
// Phase 1: database integrity and connection
info!("startup: initialising database");
match crate::db::init_db().await {
    Ok(conn) => {
        // Build AppState and inject into managed state
        let state = crate::state::AppState::new(conn);
        app.manage(state);
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
```

Add `use crate::state;` to the imports at the top of startup.rs.

─────────────────────────────────────────────────────────────────────────
STEP 4 — Declare new modules in lib.rs
─────────────────────────────────────────────────────────────────────────
Add to src-tauri/src/lib.rs (after existing mod declarations):
  mod state;

─────────────────────────────────────────────────────────────────────────
STEP 5 — Update health_check command to demonstrate state injection
─────────────────────────────────────────────────────────────────────────
This verifies the injection path works end-to-end. In
src-tauri/src/commands/mod.rs, update health_check:

```rust
use tauri::State;
use crate::state::AppState;
use crate::errors::AppResult;

#[derive(serde::Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
    pub db_connected: bool,
    pub locale: String,
}

#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> AppResult<HealthCheckResponse> {
    let config = state.config.read().await;
    Ok(HealthCheckResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_connected: true, // if we got here, the pool is live
        locale: config.default_locale.clone(),
    })
}
```

Update shared/ipc-types.ts to match the expanded response:

```typescript
export interface HealthCheckResponse {
  status: "ok" | "degraded";
  version: string;
  db_connected: boolean;
  locale: string;
}
```

─────────────────────────────────────────────────────────────────────────
STEP 6 — Validation
─────────────────────────────────────────────────────────────────────────
  cd src-tauri && cargo check --all-features
  pnpm run type-check

Both must pass. Confirm `health_check` now returns `db_connected: true`
and `locale: "fr"` when called from the browser DevTools console via
`window.__TAURI__.invoke("health_check")`.
```

---

### Supervisor Verification Checklist — S1

**State injection**
- [ ] Open DevTools Console. Run `window.__TAURI__.invoke("health_check")`. The returned object must contain: `{ status: "ok", version: "0.1.0", db_connected: true, locale: "fr" }`.  
- [ ] Confirm `src-tauri/src/state.rs` is committed and does not use any global statics (`static`, `lazy_static`, `once_cell::sync::Lazy` at module level).  
- [ ] Check `startup.rs` — `app.manage(state)` is called once, on the happy path only. There is no second `manage()` call anywhere in the codebase (Tauri panics on double-manage).

**Compile clean**
- [ ] `cargo check --all-features` — zero errors.  
- [ ] `pnpm run type-check` — zero errors.

**Acceptance sign-off:** sign supervisor feedback issue with label `sprint:S1-phase1-02b`.

---

## Sprint S2 — Background Task Supervisor and Graceful Shutdown

### Context

The Rust application core will eventually host several long-running background tasks: the sync service, the update checker, the notification dispatcher, the analytics computation engine, and the backup helper. If any of these are spawned as raw `tokio::spawn()` calls with no tracking, there is no clean shutdown path and no way to observe their status from the UI.

This sprint implements a `BackgroundTaskSupervisor` that owns all spawned tasks through `tokio::task::JoinHandle` references keyed by a stable task identifier. The supervisor supports: spawn (fire and track), graceful shutdown (broadcast a cancellation token to all running tasks), and status reporting (which tasks are alive). The shutdown sequence is hooked into the Tauri `on_window_event` for `WindowEvent::Destroyed` so that the process exits cleanly rather than leaving unfinished writes or network calls orphaned.

---

### AI Agent Prompt — S2

```
You are an expert async Rust engineer continuing work on Maintafox Desktop.
Sprint S1 complete: AppState is defined (db, config, session stub) and wired
into managed state via startup.rs.

YOUR TASK: Implement the background task supervisor and graceful shutdown.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/background/mod.rs
─────────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/background/mod.rs
//! Background task supervisor.
//!
//! Rules:
//!   - All long-running background work is spawned through this supervisor.
//!   - Each task receives a CancellationToken it must poll regularly.
//!   - Graceful shutdown broadcasts cancellation and joins all handles with a timeout.
//!   - Task identifiers are stable strings (e.g. "sync", "updater", "analytics").

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub type TaskId = &'static str;

/// Status of a tracked background task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Cancelled,
    Finished,
}

/// A tracked background task entry.
struct TaskEntry {
    handle: JoinHandle<()>,
    token: CancellationToken,
}

/// Top-level supervisor that owns all background task handles.
///
/// Clone is cheap; the inner state is Arc-wrapped.
#[derive(Clone)]
pub struct BackgroundTaskSupervisor {
    tasks: Arc<Mutex<HashMap<&'static str, TaskEntry>>>,
    /// Parent token — cancelling this cancels all child tokens.
    shutdown_token: CancellationToken,
}

impl BackgroundTaskSupervisor {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            shutdown_token: CancellationToken::new(),
        }
    }

    /// Spawn a new background task identified by `id`.
    ///
    /// If a task with the same id is already running, the spawn is refused and
    /// a warning is logged. The future receives a child cancellation token.
    pub async fn spawn<F, Fut>(&self, id: TaskId, factory: F)
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(id) {
            warn!("background: task '{id}' is already running; new spawn refused");
            return;
        }

        let child_token = self.shutdown_token.child_token();
        let token_for_task = child_token.clone();
        let handle = tokio::spawn(async move {
            info!("background task started: {id}");
            factory(token_for_task).await;
            info!("background task finished: {id}");
        });

        tasks.insert(id, TaskEntry { handle, token: child_token });
        info!("background: spawned task '{id}'");
    }

    /// Cancel a specific task by id.
    pub async fn cancel(&self, id: TaskId) {
        let mut tasks = self.tasks.lock().await;
        if let Some(entry) = tasks.get(id) {
            entry.token.cancel();
            info!("background: cancellation signalled for task '{id}'");
        } else {
            warn!("background: cancel requested for unknown task '{id}'");
        }
        tasks.remove(id);
    }

    /// Return a list of (id, status) for all known tasks.
    pub async fn status(&self) -> Vec<(String, TaskStatus)> {
        let tasks = self.tasks.lock().await;
        tasks
            .iter()
            .map(|(id, entry)| {
                let status = if entry.handle.is_finished() {
                    TaskStatus::Finished
                } else if entry.token.is_cancelled() {
                    TaskStatus::Cancelled
                } else {
                    TaskStatus::Running
                };
                (id.to_string(), status)
            })
            .collect()
    }

    /// Graceful shutdown: cancel all tasks and await them with a timeout.
    ///
    /// Called from the Tauri on_window_event Destroyed handler.
    pub async fn shutdown(&self, timeout_secs: u64) {
        info!("background: initiating graceful shutdown (timeout={timeout_secs}s)");
        self.shutdown_token.cancel();

        let deadline = tokio::time::Instant::now()
            + tokio::time::Duration::from_secs(timeout_secs);

        let mut tasks = self.tasks.lock().await;
        for (id, entry) in tasks.drain() {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, entry.handle).await {
                Ok(Ok(())) => info!("background: task '{id}' shutdown cleanly"),
                Ok(Err(e)) => error!("background: task '{id}' panicked: {e}"),
                Err(_) => warn!("background: task '{id}' did not finish within timeout"),
            }
        }

        info!("background: shutdown complete");
    }
}

impl Default for BackgroundTaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}
```

─────────────────────────────────────────────────────────────────────────
STEP 2 — Add Cargo.toml dependency: tokio-util
─────────────────────────────────────────────────────────────────────────
In src-tauri/Cargo.toml:
  tokio-util = { version = "0.7", features = ["rt"] }

The cancellation-token API is in tokio-util; it is not bundled with tokio.

─────────────────────────────────────────────────────────────────────────
STEP 3 — Add supervisor to AppState
─────────────────────────────────────────────────────────────────────────
Update src-tauri/src/state.rs. Add:

```rust
use crate::background::BackgroundTaskSupervisor;
```

Add to the AppState struct:
```rust
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<RwLock<AppConfig>>,
    pub session: Arc<RwLock<SessionManagerStub>>,
    /// Background task supervisor. Clone is cheap (Arc inside).
    pub tasks: BackgroundTaskSupervisor,
}
```

Update AppState::new:
```rust
pub fn new(db: DbPool) -> Self {
    Self {
        db,
        config: Arc::new(RwLock::new(AppConfig::default())),
        session: Arc::new(RwLock::new(SessionManagerStub::default())),
        tasks: BackgroundTaskSupervisor::new(),
    }
}
```

─────────────────────────────────────────────────────────────────────────
STEP 4 — Wire graceful shutdown into lib.rs
─────────────────────────────────────────────────────────────────────────
Tauri fires WindowEvent::Destroyed when the main window is destroyed
(only possible when we actually quit from the tray). Hook shutdown there.

In lib.rs, inside the `.setup()` closure, after `window::restore_window_state`:

```rust
// Graceful shutdown on main window destruction
let shutdown_handle = app.handle().clone();
app.get_webview_window("main")
    .expect("main window must exist")
    .on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let h = shutdown_handle.clone();
            tauri::async_runtime::block_on(async move {
                if let Some(state) = h.try_state::<crate::state::AppState>() {
                    state.tasks.shutdown(5).await;
                }
            });
        }
    });
```

Note: `window::restore_window_state` already registers a CloseRequested
handler for minimize-to-tray. Destroyed only fires on actual process exit
after the tray Quit action. The two event handlers are independent.

─────────────────────────────────────────────────────────────────────────
STEP 5 — Declare background module in lib.rs
─────────────────────────────────────────────────────────────────────────
Add to the mod declarations in lib.rs:
  mod background;

─────────────────────────────────────────────────────────────────────────
STEP 6 — Validation
─────────────────────────────────────────────────────────────────────────
  cd src-tauri && cargo check --all-features

Zero errors. The supervisor is not spawning any real tasks yet (that begins
in Sub-phase 04+). This step only proves the wiring compiles correctly.
```

---

### Supervisor Verification Checklist — S2

**Background supervisor wiring**
- [ ] `cargo check --all-features` — zero errors.  
- [ ] Open `src-tauri/src/state.rs` — confirm `AppState` has a `tasks: BackgroundTaskSupervisor` field.  
- [ ] Open `src-tauri/src/lib.rs` — confirm there is exactly one `on_window_event` handler for `Destroyed` and it calls `state.tasks.shutdown(5)`.  
- [ ] Launch the app, then quit from the tray. In the Rust console logs (`RUST_LOG=info pnpm tauri dev`), the last log lines must include `background: initiating graceful shutdown` and `background: shutdown complete`.

**Acceptance sign-off:** sign supervisor feedback issue with label `sprint:S2-phase1-02b`.

---

## Sprint S3 — IPC Command Expansion and Command Registry Update

### Context

The IPC command boundary is the only bridge between the React presentation layer and the Rust application core. Every command must be: typed (Rust struct ↔ TypeScript interface via `serde`), minimal in scope (one well-defined action), listed in `docs/IPC_COMMAND_REGISTRY.md`, and exposed with a matching service function in `src/services/` on the TypeScript side.

This sprint adds two new application-level commands designed for the startup experience and diagnostics: `get_app_info` (returns version, platform, build mode) and `get_task_status` (returns the current background supervisor status). These are the first commands that use the full `AppState` injection pattern established in S1, proving the pattern is production-ready.

---

### AI Agent Prompt — S3

```
You are an expert Rust and TypeScript engineer continuing work on Maintafox Desktop.
AppState is fully wired (S1). BackgroundTaskSupervisor is in AppState (S2).

YOUR TASK: Add two IPC commands, update the registry, and write the TypeScript service layer.

─────────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/commands/app.rs
─────────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/commands/app.rs
//! Application-level IPC commands.
//! All responses are typed structs that serialize to JSON for the TypeScript
//! layer. Command names must match entries in docs/IPC_COMMAND_REGISTRY.md.

use tauri::State;

use crate::errors::AppResult;
use crate::state::AppState;

/// Platform and build information returned by get_app_info.
#[derive(serde::Serialize)]
pub struct AppInfoResponse {
    pub version: String,
    pub build_mode: String,
    pub os: String,
    pub arch: String,
    pub app_name: String,
    pub default_locale: String,
}

/// Returns static build metadata and runtime environment info.
/// This command is always callable even before session authentication.
#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfoResponse> {
    let config = state.config.read().await;
    Ok(AppInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_mode: if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        },
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        app_name: config.app_name.clone(),
        default_locale: config.default_locale.clone(),
    })
}

/// One entry in the background task status list.
#[derive(serde::Serialize)]
pub struct TaskStatusEntry {
    pub id: String,
    pub status: crate::background::TaskStatus,
}

/// Returns the current status of all tracked background tasks.
/// Supervisor returns empty list before any tasks are spawned (Phase 1 normal state).
#[tauri::command]
pub async fn get_task_status(
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskStatusEntry>> {
    let entries = state.tasks.status().await;
    Ok(entries
        .into_iter()
        .map(|(id, status)| TaskStatusEntry { id, status })
        .collect())
}
```

─────────────────────────────────────────────────────────────────────────
STEP 2 — Register commands in src-tauri/src/commands/mod.rs
─────────────────────────────────────────────────────────────────────────
Update mod.rs:
```rust
// src-tauri/src/commands/mod.rs
pub mod app;

use tauri::State;
use crate::state::AppState;
use crate::errors::AppResult;

#[derive(serde::Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
    pub db_connected: bool,
    pub locale: String,
}

#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> AppResult<HealthCheckResponse> {
    let config = state.config.read().await;
    Ok(HealthCheckResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_connected: true,
        locale: config.default_locale.clone(),
    })
}

pub use app::get_app_info;
pub use app::get_task_status;
```

─────────────────────────────────────────────────────────────────────────
STEP 3 — Register commands in lib.rs invoke_handler
─────────────────────────────────────────────────────────────────────────
Update the invoke_handler in lib.rs:
```rust
.invoke_handler(tauri::generate_handler![
    commands::health_check,
    commands::get_app_info,
    commands::get_task_status,
])
```

─────────────────────────────────────────────────────────────────────────
STEP 4 — Add TypeScript types to shared/ipc-types.ts
─────────────────────────────────────────────────────────────────────────
Append to shared/ipc-types.ts:

```typescript
// ─── App Info ──────────────────────────────────────────────────────────────

export interface AppInfoResponse {
  version: string;
  build_mode: "debug" | "release";
  os: string;
  arch: string;
  app_name: string;
  default_locale: string;
}

// ─── Task Status ───────────────────────────────────────────────────────────

export type TaskStatusKind = "running" | "cancelled" | "finished";

export interface TaskStatusEntry {
  id: string;
  status: TaskStatusKind;
}
```

─────────────────────────────────────────────────────────────────────────
STEP 5 — Create src/services/app.service.ts
─────────────────────────────────────────────────────────────────────────
All frontend IPC calls must go through src/services/. Direct use of
invoke() outside of services/ is forbidden (see CODING_STANDARDS_FRONTEND.md).

```typescript
// src/services/app.service.ts
import { invoke } from "@tauri-apps/api/core";
import type { AppInfoResponse, HealthCheckResponse, TaskStatusEntry } from "@shared/ipc-types";

/**
 * Health check — confirms the IPC bridge and DB are live.
 * Called once during startup sequence listening.
 */
export async function healthCheck(): Promise<HealthCheckResponse> {
  return invoke<HealthCheckResponse>("health_check");
}

/**
 * Returns application build info and runtime environment.
 */
export async function getAppInfo(): Promise<AppInfoResponse> {
  return invoke<AppInfoResponse>("get_app_info");
}

/**
 * Returns the status of all tracked background tasks.
 * Returns an empty array in Phase 1 (no tasks spawned yet).
 */
export async function getTaskStatus(): Promise<TaskStatusEntry[]> {
  return invoke<TaskStatusEntry[]>("get_task_status");
}
```

─────────────────────────────────────────────────────────────────────────
STEP 6 — Update docs/IPC_COMMAND_REGISTRY.md
─────────────────────────────────────────────────────────────────────────
The registry must list every IPC command. Update the existing entry for
health_check and add the two new commands:

## IPC Command Registry

> Source of truth for all Tauri IPC commands. Every new command in
> invoke_handler MUST have an entry here. Entries use snake_case and
> match the Rust #[tauri::command] function name exactly.

| Command | Rust handler | Auth required | AppState fields used | TypeScript service |
|---------|-------------|--------------|---------------------|--------------------|
| `health_check` | `commands::health_check` | No | `config` (read) | `app.service.ts::healthCheck` |
| `get_app_info` | `commands::app::get_app_info` | No | `config` (read) | `app.service.ts::getAppInfo` |
| `get_task_status` | `commands::app::get_task_status` | No | `tasks` (read) | `app.service.ts::getTaskStatus` |

### Rules
1. Auth-required commands must validate the session before accessing any app data.
   Sub-phase 04 adds the `require_session!()` macro enforced at the top of each guarded handler.
2. Commands must NEVER return database entity objects directly. Always define a dedicated response struct.
3. All new IPC commands must be added here before they are merged to develop.

─────────────────────────────────────────────────────────────────────────
STEP 7 — Update CHANGELOG.md [Unreleased]
─────────────────────────────────────────────────────────────────────────
Append under [Unreleased] → Added in CHANGELOG.md:
- Rust core: AppState struct (db pool, config cache, session stub, task supervisor)
- Rust core: BackgroundTaskSupervisor with spawn, cancel, status, and graceful shutdown
- Rust core: IPC commands get_app_info and get_task_status
- Rust core: Graceful shutdown hooked into Tauri WindowEvent::Destroyed
- Frontend: src/services/app.service.ts (health_check, get_app_info, get_task_status wrappers)
- Docs: IPC_COMMAND_REGISTRY.md updated to 3 commands

─────────────────────────────────────────────────────────────────────────
STEP 8 — Validation
─────────────────────────────────────────────────────────────────────────
  cd src-tauri && cargo check --all-features
  pnpm run type-check
  pnpm run lint

All pass with zero errors. Verify in DevTools console:
  await window.__TAURI__.invoke("get_app_info")
  // Expected: { version:"0.1.0", build_mode:"debug", os:"windows", ... }
  await window.__TAURI__.invoke("get_task_status")
  // Expected: [] (empty array — no tasks spawned in Phase 1)
```

---

### Supervisor Verification Checklist — S3

**IPC commands**
- [ ] DevTools Console: `await window.__TAURI__.invoke("get_app_info")` returns an object with `version`, `build_mode`, `os`, `arch`, `app_name`, `default_locale`.  
- [ ] DevTools Console: `await window.__TAURI__.invoke("get_task_status")` returns `[]` (empty array — correct for Phase 1).  
- [ ] Confirm there is no `invoke()` call anywhere in `src/` outside of `src/services/`. Run: `grep -r "invoke(" src/ --include="*.ts" --include="*.tsx" | grep -v "services/"` — must return zero results.

**Registry**
- [ ] Open `docs/IPC_COMMAND_REGISTRY.md` — must contain exactly 3 commands: `health_check`, `get_app_info`, `get_task_status`.  
- [ ] Every command in `lib.rs::invoke_handler` appears in the registry. Every row in the registry corresponds to a real command in the handler (no orphans in either direction).

**Service boundary**
- [ ] `src/services/app.service.ts` exists and exports `healthCheck`, `getAppInfo`, `getTaskStatus`.  
- [ ] Both functions use the correct response types imported from `@shared/ipc-types`.

**Compile clean**
- [ ] `cargo check --all-features` — zero errors.  
- [ ] `pnpm run type-check` — zero errors.  
- [ ] `pnpm run lint` — zero warnings.

**Acceptance sign-off:** sign supervisor feedback issue with label `sprint:S3-phase1-02b` before proceeding to File 03.

---

## File 02 — Completion Summary

By the end of this file the following permanent artifacts exist or are updated:

| Artifact | Location | Purpose |
|----------|----------|---------|
| `state.rs` | `src-tauri/src/` | AppState: db pool, config cache, session stub, task supervisor |
| `background/mod.rs` | `src-tauri/src/` | BackgroundTaskSupervisor: spawn, cancel, status, shutdown |
| `commands/app.rs` | `src-tauri/src/` | get_app_info and get_task_status command handlers |
| `commands/mod.rs` | `src-tauri/src/` | Updated: re-exports all 3 commands |
| `lib.rs` | `src-tauri/src/` | Updated: declares background+state modules, shutdown hook, 3 commands in handler |
| `startup.rs` | `src-tauri/src/` | Updated: builds AppState from db pool, calls app.manage() |
| `db/mod.rs` | `src-tauri/src/` | Updated: init_db returns DatabaseConnection (WAL+FK), run_migrations stub |
| `app.service.ts` | `src/services/` | TypeScript service wrapping 3 IPC commands |
| `ipc-types.ts` | `shared/` | Updated: AppInfoResponse, TaskStatusEntry types |
| `IPC_COMMAND_REGISTRY.md` | `docs/` | Updated: 3 commands documented |

**Trust boundary summary:** The Rust application core now has a properly injected AppState, a supervised background task plane, and a minimal but complete IPC command set. All frontend-to-backend calls are narrow, typed, and registered. The session manager is a stub pending Sub-phase 04. No task is spawned in Phase 1 — the supervisor is ready but idle, which is the correct empty state.

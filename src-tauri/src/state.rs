//! Application-wide shared state injected into all Tauri IPC commands.
//!
//! Rules:
//!   - `AppState` is immutable after initialization (Arc wraps mutable sub-components).
//!   - No global statics — all access is through `tauri::State<AppState>`.
//!   - Session manager holds the active user session (login/logout/idle-lock).

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::auth::session_manager::SessionManager;
use crate::background::BackgroundTaskSupervisor;

/// Database connection pool managed by sea-orm.
/// Concrete type is `sea_orm::DatabaseConnection`; `SQLite` in WAL mode.
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

/// Central application state shared across all IPC commands.
///
/// Obtain via `tauri::State<AppState>` in command handlers.
/// Components are wrapped in `Arc<RwLock<>>` only where mutation after init is needed;
/// the db pool and config are read-heavy and use a simpler shared reference model.
#[derive(Debug)]
pub struct AppState {
    /// Live database connection pool. Never clone the pool; always use `&self.db`.
    pub db: DbPool,
    /// Application configuration cache. `Arc<RwLock<>>` so Settings module can hot-reload.
    pub config: Arc<RwLock<AppConfig>>,
    /// Session manager: owns the in-memory session and enforces expiry/idle-lock.
    pub session: Arc<RwLock<SessionManager>>,
    /// Background task supervisor. Clone is cheap (Arc inside).
    pub tasks: BackgroundTaskSupervisor,
}

impl AppState {
    /// Construct from a database connection. Config and session start at defaults.
    pub fn new(db: DbPool) -> Self {
        Self {
            db,
            config: Arc::new(RwLock::new(AppConfig::default())),
            session: Arc::new(RwLock::new(SessionManager::new())),
            tasks: BackgroundTaskSupervisor::new(),
        }
    }
}

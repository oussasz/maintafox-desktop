//! Audit event writer.
//!
//! All security-significant operations emit an audit event. Audit writes are
//! fire-and-forget: if the insert fails, a `tracing::error!` is emitted but the
//! operation is **NOT** blocked. Downtime is never caused by audit failure.
//!
//! The `audit_events` table (migration 001) has columns:
//!   `id` TEXT PK, `event_type`, `actor_id`, `actor_name`, `entity_type`,
//!   `entity_id`, `summary`, `detail_json`, `device_id`, `occurred_at`.
//!
//! Callers: use the free function [`emit`] for awaited writes, or
//! [`emit_background`] for fire-and-forget Tokio-spawned writes.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tracing::error;

/// All recognized audit event types.
/// Use these string constants to avoid typos in `event_type` values.
pub mod event_type {
    pub const LOGIN_SUCCESS: &str = "login.success";
    pub const LOGIN_FAILURE: &str = "login.failure";
    pub const LOGIN_OFFLINE: &str = "login.offline";
    pub const LOGOUT: &str = "logout";
    pub const SESSION_EXPIRED: &str = "session.expired";
    pub const SESSION_IDLE_LOCKED: &str = "session.idle_locked";
    pub const DEVICE_TRUST_REGISTERED: &str = "device.trust_registered";
    pub const DEVICE_TRUST_REVOKED: &str = "device.trust_revoked";
    pub const STEP_UP_SUCCESS: &str = "step_up.success";
    pub const STEP_UP_FAILURE: &str = "step_up.failure";
    pub const PERMISSION_DENIED: &str = "permission.denied";
    pub const ROLE_ASSIGNED: &str = "role.assigned";
    pub const ROLE_REVOKED: &str = "role.revoked";
    pub const PERMISSION_GRANTED: &str = "permission.granted";
    pub const PERMISSION_REMOVED: &str = "permission.removed";
    pub const USER_CREATED: &str = "user.created";
    pub const USER_DEACTIVATED: &str = "user.deactivated";
    pub const PASSWORD_CHANGED: &str = "user.password_changed";
    pub const FORCE_CHANGE_SET: &str = "user.force_change_set";
}

/// Builder for audit event parameters.
///
/// Only `event_type` is semantically required — all other fields fall back to
/// `None` / empty via [`Default`].
#[derive(Debug, Default)]
pub struct AuditEvent<'a> {
    pub event_type: &'a str,
    pub actor_id: Option<i32>,
    pub actor_name: Option<&'a str>,
    pub entity_type: Option<&'a str>,
    pub entity_id: Option<&'a str>,
    pub summary: &'a str,
    pub detail_json: Option<String>,
    pub device_id: Option<&'a str>,
}

/// Emit an audit event (awaited).
///
/// Any DB error is logged at `error!` level but **never propagated** — the
/// caller always succeeds regardless of audit write outcome.
pub async fn emit(db: &DatabaseConnection, event: AuditEvent<'_>) {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO audit_events
                   (id, event_type, actor_id, actor_name, entity_type,
                    entity_id, summary, detail_json, device_id, occurred_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                id.into(),
                event.event_type.into(),
                event
                    .actor_id
                    .map_or(sea_orm::Value::String(None), |v| v.to_string().into()),
                event
                    .actor_name
                    .map_or(sea_orm::Value::String(None), |s| s.to_string().into()),
                event
                    .entity_type
                    .map_or(sea_orm::Value::String(None), |s| s.to_string().into()),
                event
                    .entity_id
                    .map_or(sea_orm::Value::String(None), |s| s.to_string().into()),
                event.summary.into(),
                event
                    .detail_json
                    .clone()
                    .map_or(sea_orm::Value::String(None), Into::into),
                event
                    .device_id
                    .map_or(sea_orm::Value::String(None), |s| s.to_string().into()),
                now.into(),
            ],
        ))
        .await;

    if let Err(e) = result {
        error!(event_type = event.event_type, error = %e, "audit::emit failed");
    }
}

//! Audit module.
//!
//! `writer::write_audit_event` is the strict append-only writer that propagates
//! persistence failures so callers can decide whether to block.
//! `emit` remains as a compatibility adapter for existing call sites that use
//! fire-and-log semantics.

pub mod writer;

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
    pub const PIN_SET: &str = "user.pin_set";
    pub const PIN_CLEARED: &str = "user.pin_cleared";
    pub const SESSION_UNLOCKED_WITH_PIN: &str = "session.unlocked_with_pin";
    pub const RBAC_EMERGENCY_GRANT_CREATED: &str = "rbac.emergency_grant_created";
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
pub async fn emit(db: &sea_orm::DatabaseConnection, event: AuditEvent<'_>) {
    let details_json = build_compat_details_json(&event);
    let input = writer::AuditEventInput {
        action_code: event.event_type.to_string(),
        target_type: event.entity_type.map(ToOwned::to_owned),
        target_id: event.entity_id.map(ToOwned::to_owned),
        actor_id: event.actor_id.map(i64::from),
        auth_context: infer_auth_context(event.event_type).to_string(),
        result: infer_result(event.event_type).to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(details_json),
    };

    if let Err(err) = writer::write_audit_event(db, input).await {
        tracing::error!(
            event_type = event.event_type,
            error = %err,
            "audit::emit compatibility writer failed"
        );
    }
}

fn infer_auth_context(event_type: &str) -> &'static str {
    if event_type.contains("pin") {
        "pin"
    } else if event_type.contains("step_up") {
        "step_up"
    } else if event_type.contains("login") || event_type.contains("password") {
        "password"
    } else {
        "system"
    }
}

fn infer_result(event_type: &str) -> &'static str {
    if event_type.contains("failure") {
        "fail"
    } else if event_type == event_type::PERMISSION_DENIED {
        "blocked"
    } else {
        "success"
    }
}

fn build_compat_details_json(event: &AuditEvent<'_>) -> serde_json::Value {
    let mut details = serde_json::json!({
        "summary": event.summary,
        "actor_name": event.actor_name,
        "device_id": event.device_id,
    });

    if let Some(raw) = &event.detail_json {
        let parsed = serde_json::from_str::<serde_json::Value>(raw)
            .unwrap_or_else(|_| serde_json::json!({ "raw_detail_json": raw }));
        if let Some(obj) = details.as_object_mut() {
            obj.insert("detail".to_string(), parsed);
        }
    }

    details
}

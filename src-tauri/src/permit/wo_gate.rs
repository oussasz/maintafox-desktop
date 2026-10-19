//! Permit gate of work order execution: an OT starts, resumes or is reopened to in-progress only
//! when every live permit linked to it is active and ready to protect the work (not expired, LOTO
//! isolations all applied and verified, none removed) — whether or not the OT is flagged as
//! requiring a permit — and, when it requires one, at least one exists; when a plant rule flagged it
//! at conversion (and the flag was not cleared), at least one is of the rule's permit type. A live
//! permit that expires before the work order's planned end refuses the start too (owner decision:
//! the work would outlive its protection); work already in progress is only warned (planning and
//! the expiry notification). Planning only warns about permits; this gate is where the rule is
//! hard-enforced. Callers run it inside the transaction that changes the status.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};

use super::queries::{activation_ready, list_live_permits_linked_to_work_order};

pub async fn assert_in_progress_permit_gate(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_permit, planned_end, \
             CASE WHEN permit_rule_cleared_at IS NULL THEN permit_rule_permit_type END AS rule_permit_type \
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let requires: i64 = row
        .try_get("", "requires_permit")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("requires_permit decode: {e}")))?;
    let rule_permit_type: Option<String> = row
        .try_get("", "rule_permit_type")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("rule_permit_type decode: {e}")))?;
    let planned_end: Option<String> = row
        .try_get("", "planned_end")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("planned_end decode: {e}")))?;
    let required = requires == 1;
    assert_live_permits_ready(
        db,
        wo_id,
        required,
        rule_permit_type.filter(|_| required).as_deref(),
        planned_end.as_deref().map(str::trim).filter(|e| !e.is_empty()),
    )
    .await
}

/// Stable code: a live permit of the work order expires before its planned end.
pub const CODE_EXPIRES_BEFORE_PLANNED_END: &str = "PERMIT_EXPIRES_BEFORE_PLANNED_END";

/// The permit expires before the work order's planned end (both read as instants; an unreadable
/// time is refused rather than guessed). A work order without a planned end is not judged.
fn expires_before_planned_end(permit_code: &str, expires_at: Option<&str>, planned_end: Option<&str>) -> AppResult<()> {
    let (Some(expiry_raw), Some(end_raw)) = (expires_at.map(str::trim).filter(|e| !e.is_empty()), planned_end) else {
        return Ok(());
    };
    let (Some(expiry), Some(end)) = (
        crate::settings::site::instant(expiry_raw),
        crate::settings::site::instant(end_raw),
    ) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Permit {permit_code}: its expiry ({expiry_raw}) or the work order's planned end ({end_raw}) is not a readable time."
        )]));
    };
    if expiry < end {
        return Err(org_validation_failed(
            CODE_EXPIRES_BEFORE_PLANNED_END,
            format!(
                "Permit {permit_code} expires ({expiry_raw}) before the work order's planned end ({end_raw}): issue a permit that covers the planned work, or re-plan the work, before starting or resuming it."
            ),
            HashMap::from([
                ("permit".to_string(), permit_code.to_string()),
                ("expires_at".to_string(), expiry_raw.to_string()),
                ("planned_end".to_string(), end_raw.to_string()),
            ]),
        ));
    }
    Ok(())
}

/// Every live permit linked to the work order is active and ready (a permit that exists protects
/// the work, flag or not); when the OT requires a permit, at least one exists, and when a plant rule
/// names the permit type, at least one is of that type (a permit of another type does not protect
/// the hazard the rule is about); no live permit expires before the planned end.
async fn assert_live_permits_ready(
    db: &impl ConnectionTrait,
    wo_id: i64,
    required: bool,
    rule_permit_type: Option<&str>,
    planned_end: Option<&str>,
) -> AppResult<()> {
    let permits = list_live_permits_linked_to_work_order(db, wo_id).await?;
    if permits.is_empty() && required {
        return Err(org_validation_failed(
            "PERMIT_REQUIRED_TO_START",
            "This work order requires a work permit: link an active permit before starting or resuming it.",
            HashMap::new(),
        ));
    }
    if let Some(permit_type) = rule_permit_type {
        if !permits
            .iter()
            .any(|p| p.permit_type_code.as_deref() == Some(permit_type))
        {
            let name = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT name FROM permit_types WHERE code = ?",
                    [permit_type.into()],
                ))
                .await?
                .and_then(|r| r.try_get::<String>("", "name").ok())
                .unwrap_or_else(|| permit_type.to_string());
            return Err(org_validation_failed(
                "PERMIT_TYPE_REQUIRED_TO_START",
                format!(
                    "A permit rule requires a {name} permit for this work order: link an active permit of that type before starting or resuming it."
                ),
                HashMap::from([
                    ("permit_type".to_string(), permit_type.to_string()),
                    ("permit_type_name".to_string(), name),
                ]),
            ));
        }
    }
    for p in &permits {
        let params = || {
            HashMap::from([
                ("permit".to_string(), p.code.clone()),
                ("status".to_string(), p.status.clone()),
            ])
        };
        if p.status == "suspended" || p.status == "revalidation_required" {
            return Err(org_validation_failed(
                "PERMIT_SUSPENDED",
                format!(
                    "Permit {} is suspended: revalidate it before starting or resuming the work.",
                    p.code
                ),
                params(),
            ));
        }
        if p.status != "active" {
            return Err(org_validation_failed(
                "PERMIT_NOT_ACTIVE",
                format!(
                    "Permit {} is '{}': it must be active before the work starts or resumes.",
                    p.code, p.status
                ),
                params(),
            ));
        }
        activation_ready(db, p).await?;
        expires_before_planned_end(&p.code, p.expires_at.as_deref(), planned_end)?;
    }
    Ok(())
}

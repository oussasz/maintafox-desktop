//! Permit gate of work order execution: an OT starts, resumes or is reopened to in-progress only
//! when every live permit linked to it is active and ready to protect the work (not expired, LOTO
//! isolations all applied and verified, none removed) — whether or not the OT is flagged as
//! requiring a permit — and, when it requires one, at least one exists. Planning only warns about
//! permits; this gate is where the rule is hard-enforced. Callers run it inside the transaction
//! that changes the status.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};

use super::queries::{activation_ready, list_live_permits_linked_to_work_order};

pub async fn assert_in_progress_permit_gate(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_permit FROM work_orders WHERE id = ?",
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
    assert_live_permits_ready(db, wo_id, requires == 1).await
}

/// Every live permit linked to the work order is active and ready (a permit that exists protects
/// the work, flag or not); when the OT requires a permit, at least one exists.
async fn assert_live_permits_ready(db: &impl ConnectionTrait, wo_id: i64, required: bool) -> AppResult<()> {
    let permits = list_live_permits_linked_to_work_order(db, wo_id).await?;
    if permits.is_empty() && required {
        return Err(org_validation_failed(
            "PERMIT_REQUIRED_TO_START",
            "This work order requires a work permit: link an active permit before starting or resuming it.",
            HashMap::new(),
        ));
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
    }
    Ok(())
}

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::sync_stage::stage_work_order_sync;

use super::domain::WorkPermit;
use super::queries::{activation_ready, get_work_permit_linked_to_work_order, stage_work_permit_batched};

pub async fn assert_in_progress_permit_gate(db: &DatabaseConnection, wo_id: i64) -> AppResult<()> {
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
    let requires: i64 = row.try_get("", "requires_permit").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("requires_permit decode: {e}"))
    })?;
    if requires == 0 {
        return Ok(());
    }
    let permit = get_work_permit_linked_to_work_order(db, wo_id).await?;
    let Some(p) = permit else {
        return Err(AppError::ValidationFailed(vec![
            "Un permis de travail actif lié est requis pour passer cet ordre de travail en cours."
                .into(),
        ]));
    };
    if p.status == "suspended" {
        return Err(AppError::ValidationFailed(vec![
            "Impossible de démarrer : le permis est suspendu.".into(),
        ]));
    }
    if p.status != "active" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le permis doit être au statut « actif » pour démarrer (statut actuel : {}).",
            p.status
        )]));
    }
    activation_ready(db, p.id, p.permit_type_id).await?;
    Ok(())
}

pub async fn stage_wo_in_progress_sync_pair(
    db: &DatabaseConnection,
    wo: &WorkOrder,
    permit: &WorkPermit,
    batch_id: &str,
) -> AppResult<()> {
    let status_code = wo.status_code.as_deref().unwrap_or("in_progress");
    let type_code = wo.type_code.as_deref().unwrap_or("corrective");
    stage_work_order_sync(
        db,
        wo,
        status_code,
        type_code,
        wo.closed_at.as_deref(),
        wo.closeout_validation_profile_id,
        wo.closeout_validation_passed,
    )
    .await?;
    stage_work_permit_batched(db, permit, batch_id).await
}

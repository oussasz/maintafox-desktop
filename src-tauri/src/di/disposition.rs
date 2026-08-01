//! DI disposition validation and close helpers (lifecycle redesign).

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub const DISPOSITION_CONVERTED_TO_WO: &str = "converted_to_wo";
pub const DISPOSITION_REJECTED_INVALID: &str = "rejected_invalid";
pub const DISPOSITION_DUPLICATE: &str = "duplicate";
pub const DISPOSITION_CANCELLED_BY_REQUESTER: &str = "cancelled_by_requester";
pub const DISPOSITION_CANCELLED_BY_PLANNER: &str = "cancelled_by_planner";
pub const DISPOSITION_NO_WORK_REQUIRED: &str = "no_work_required";
pub const DISPOSITION_SOLVED_IMMEDIATELY: &str = "solved_immediately";
pub const DISPOSITION_INFORMATION_ONLY: &str = "information_only";
pub const DISPOSITION_OTHER: &str = "other";

const CLOSE_DISPOSITIONS: &[&str] = &[
    DISPOSITION_REJECTED_INVALID,
    DISPOSITION_DUPLICATE,
    DISPOSITION_CANCELLED_BY_PLANNER,
    DISPOSITION_NO_WORK_REQUIRED,
    DISPOSITION_SOLVED_IMMEDIATELY,
    DISPOSITION_INFORMATION_ONLY,
    DISPOSITION_OTHER,
];

/// Validate a close-path disposition (not converted_to_wo — that is convert-only).
pub async fn validate_close_disposition(
    db: &impl ConnectionTrait,
    disposition_code: &str,
    notes: Option<&str>,
    related_di_id: Option<i64>,
    closing_di_id: i64,
) -> AppResult<()> {
    let code = disposition_code.trim();
    if code.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code de disposition est obligatoire pour clôturer une DI.".into(),
        ]));
    }
    if code == DISPOSITION_CONVERTED_TO_WO {
        return Err(AppError::ValidationFailed(vec![
            "La disposition 'converted_to_wo' est réservée à la conversion en OT.".into(),
        ]));
    }
    if code == DISPOSITION_CANCELLED_BY_REQUESTER {
        return Err(AppError::ValidationFailed(vec![
            "Utilisez cancel_own_di pour l'annulation demandeur.".into(),
        ]));
    }
    if !CLOSE_DISPOSITIONS.contains(&code) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Disposition inconnue ou non autorisée pour close_di: '{code}'."
        )]));
    }

    // Prefer reference catalog when present.
    super::reference_catalog::validate_di_disposition(db, code).await?;

    if code == DISPOSITION_OTHER {
        let n = notes.map(str::trim).unwrap_or("");
        if n.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Des notes sont obligatoires pour la disposition 'other'.".into(),
            ]));
        }
    }

    if code == DISPOSITION_DUPLICATE {
        let Some(related_id) = related_di_id else {
            return Err(AppError::ValidationFailed(vec![
                "related_di_id est obligatoire pour la disposition 'duplicate'.".into(),
            ]));
        };
        if related_id == closing_di_id {
            return Err(AppError::ValidationFailed(vec![
                "related_di_id ne peut pas pointer vers la DI en cours de clôture.".into(),
            ]));
        }
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM intervention_requests WHERE id = ?",
                [related_id.into()],
            ))
            .await?;
        if exists.is_none() {
            return Err(AppError::ValidationFailed(vec![format!(
                "DI liée introuvable (related_di_id={related_id})."
            )]));
        }
    }

    Ok(())
}

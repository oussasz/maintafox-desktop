//! What becomes of a commitment after it is made (migration 212, `schedule_commitments.status`):
//!
//! - `committed`: it holds its slot (the only status that loads crews and technicians).
//! - `released` ("Créneau libéré / Slot released"): the planner gives the slot back; the job returns
//!   to the backlog to be planned again. History kept.
//! - `cancelled`: the job is taken off the schedule (the work will not be done in this plan).
//! - `closed`: set automatically once its work order, PM occurrence or request is finished.
//!
//! Releasing or cancelling needs a reason and is refused on a frozen commitment: a frozen week
//! changes only through a recorded break-in. The work order's planned slot written by the
//! commitment is cleared (only while the work order is still in planning / ready and still carries
//! that slot); its crew and responsible are left as they are.

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppError, AppResult};

pub const STATUS_COMMITTED: &str = "committed";
pub const STATUS_RELEASED: &str = "released";
pub const STATUS_CANCELLED: &str = "cancelled";
pub const STATUS_CLOSED: &str = "closed";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("commitment lifecycle decode '{field}': {err}"))
}

fn now_z() -> String {
    crate::clock::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndCommitmentInput {
    pub commitment_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
}

struct CommitmentState {
    status: String,
    frozen_at: Option<String>,
    row_version: i64,
    source_type: String,
    source_id: i64,
    committed_start: String,
    committed_end: String,
}

async fn load_state(db: &impl ConnectionTrait, commitment_id: i64) -> AppResult<CommitmentState> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status, frozen_at, row_version, source_type, source_id, committed_start, committed_end \
               FROM schedule_commitments WHERE id = ?",
            [commitment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCommitment".to_string(),
            id: commitment_id.to_string(),
        })?;
    Ok(CommitmentState {
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        frozen_at: row.try_get("", "frozen_at").map_err(|e| decode_err("frozen_at", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        source_type: row
            .try_get("", "source_type")
            .map_err(|e| decode_err("source_type", e))?,
        source_id: row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?,
        committed_start: row
            .try_get("", "committed_start")
            .map_err(|e| decode_err("committed_start", e))?,
        committed_end: row
            .try_get("", "committed_end")
            .map_err(|e| decode_err("committed_end", e))?,
    })
}

/// Releases (`released`) or cancels (`cancelled`) a commitment that still holds its slot.
pub async fn end_commitment(
    db: &DatabaseConnection,
    actor_id: i64,
    input: EndCommitmentInput,
    to_status: &str,
) -> AppResult<()> {
    if to_status != STATUS_RELEASED && to_status != STATUS_CANCELLED {
        return Err(AppError::Internal(anyhow::anyhow!("end_commitment to '{to_status}'")));
    }
    let reason = input.reason.trim().to_string();
    if reason.is_empty() {
        return Err(org_validation_failed(
            "PLANNING_COMMITMENT_REASON_REQUIRED",
            "Give the reason the slot is released or the job is cancelled.",
            HashMap::new(),
        ));
    }
    let tx = db.begin().await?;
    let state = load_state(&tx, input.commitment_id).await?;
    if state.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Schedule commitment was modified elsewhere (stale row_version)."
                .to_string(),
        ]));
    }
    if state.status != STATUS_COMMITTED {
        return Err(org_validation_failed(
            "PLANNING_COMMITMENT_NOT_ACTIVE",
            format!("This commitment is already {}.", state.status),
            HashMap::from([("status".to_string(), state.status.clone())]),
        ));
    }
    if state.frozen_at.is_some() {
        return Err(org_validation_failed(
            "PLANNING_COMMITMENT_FROZEN",
            "This job is in a frozen week: it changes only through a recorded break-in.",
            HashMap::new(),
        ));
    }
    let now = now_z();
    let updated = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE schedule_commitments SET status = ?, status_reason = ?, status_changed_at = ?, \
                    status_changed_by_id = ?, row_version = row_version + 1, updated_at = ? \
              WHERE id = ? AND row_version = ? AND status = 'committed'",
            [
                to_status.into(),
                reason.clone().into(),
                now.clone().into(),
                actor_id.into(),
                now.into(),
                input.commitment_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Schedule commitment was modified elsewhere (stale row_version)."
                .to_string(),
        ]));
    }
    if state.source_type == "work_order" {
        super::wo_sync::clear_work_order_slot(
            &tx,
            actor_id,
            state.source_id,
            &state.committed_start,
            &state.committed_end,
            to_status,
        )
        .await?;
    }
    super::scheduling::append_lifecycle_log(&tx, input.commitment_id, actor_id, to_status, &reason).await?;
    tx.commit().await?;
    Ok(())
}

/// Closes the `committed` commitments matching `filter` (a condition on `schedule_commitments`
/// columns) and writes one `close_commitment` change-log row each, so the job's history says why
/// its slot ended. The close is dated at the moment the work ended (`load::finished_at_sql`), so
/// every device records the same closing time. `actor_id`: the person whose command finished the
/// work (named as who changed the status and in the change log), `None` for the automatic close.
async fn close_where(
    db: &impl ConnectionTrait,
    filter: &str,
    values: Vec<sea_orm::Value>,
    actor_id: Option<i64>,
) -> AppResult<u64> {
    let mut log_values: Vec<sea_orm::Value> = vec![actor_id.into()];
    log_values.extend(values.clone());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "INSERT INTO schedule_change_log \
                 (commitment_id, action_type, actor_id, field_changed, old_value, new_value, reason_code) \
             SELECT id, 'close_commitment', ?, 'status', 'committed', 'closed', 'work_finished' \
               FROM schedule_commitments WHERE status = 'committed' AND {filter}"
        ),
        log_values,
    ))
    .await?;
    let now = now_z();
    let finished_at = super::load::finished_at_sql("schedule_commitments");
    let mut update_values: Vec<sea_orm::Value> = vec![now.clone().into(), actor_id.into(), now.into()];
    update_values.extend(values);
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE schedule_commitments SET status = 'closed', status_changed_at = COALESCE({finished_at}, ?), \
                        status_changed_by_id = ?, row_version = row_version + 1, updated_at = ? \
                  WHERE status = 'committed' AND {filter}"
            ),
            update_values,
        ))
        .await?;
    Ok(result.rows_affected())
}

/// Closes the commitments whose work is finished (work order completed / closed / cancelled, PM
/// occurrence completed / cancelled / missed, request converted / closed / declined / archived).
/// Returns how many were closed.
///
/// The automatic close (planning tick) names nobody, and the sync server takes a commitment's
/// `status_changed_at` only from the person named in `status_changed_by` who signed it (refused
/// `author_not_signer` otherwise). Every PC closes the same commitments itself, at the same time
/// (the work's own end), so the close is written here with capture suppressed and is not sent; a
/// work order finished by a person's command closes its commitments in that person's name
/// ([`close_commitments_of_work_order`]), which travels.
pub async fn close_finished_commitments(db: &DatabaseConnection) -> AppResult<u64> {
    let active = super::load::active_commitment_sql_as("schedule_commitments");
    let tx = db.begin().await?;
    // Changes captured before it (a planner's edit not staged yet) are staged first, as they stood:
    // the automatic close is never sent inside a person's change.
    crate::sync::capture::drain_capture_journal(&tx).await?;
    crate::sync::capture::begin_suppression(&tx).await?;
    let closed = close_where(&tx, &format!("NOT {active}"), Vec::new(), None).await?;
    crate::sync::capture::end_suppression(&tx).await?;
    tx.commit().await?;
    Ok(closed)
}

/// Closes the commitments of one work order (and of the PM occurrence / request it comes from),
/// in the transaction of `actor_id`'s command that finishes it, in their name.
pub async fn close_commitments_of_work_order(db: &impl ConnectionTrait, wo_id: i64, actor_id: i64) -> AppResult<u64> {
    close_where(
        db,
        "((source_type = 'work_order' AND source_id = ?) \
           OR (source_type = 'pm_occurrence' AND source_id IN \
                  (SELECT id FROM pm_occurrences WHERE linked_work_order_id = ?)) \
           OR (source_type = 'inspection_follow_up' AND source_id IN \
                  (SELECT id FROM intervention_requests WHERE converted_to_wo_id = ?)))",
        vec![wo_id.into(), wo_id.into(), wo_id.into()],
        Some(actor_id),
    )
    .await
}

/// The candidate already holds a slot (a `committed` commitment): it cannot be committed again.
pub async fn candidate_holds_a_slot(db: &impl ConnectionTrait, candidate_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM schedule_commitments WHERE schedule_candidate_id = ? AND status = 'committed' LIMIT 1",
            [candidate_id.into()],
        ))
        .await?;
    Ok(row.is_some())
}

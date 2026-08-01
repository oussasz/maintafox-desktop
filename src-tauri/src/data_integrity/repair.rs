use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::queries::{get_finding, last_insert_rowid, DataIntegrityFindingRow};
use super::sync_stage::{stage_finding_sync, stage_repair_action_sync};

#[derive(Debug, Clone, Deserialize)]
pub struct WaiveDataIntegrityFindingInput {
    pub finding_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
    pub approver_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyDataIntegrityRepairInput {
    pub finding_id: i64,
    pub expected_row_version: i64,
    pub repair_kind: String,
}

fn map_err(col: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("data_integrity repair {col}: {e}"))
}

async fn update_finding_waived(
    txn: &impl ConnectionTrait,
    id: i64,
    expected_rv: i64,
    waiver_reason: &str,
    waiver_approver_id: i64,
) -> AppResult<()> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let aff = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE data_integrity_findings SET \
             status = 'waived', cleared_at = ?, row_version = row_version + 1, \
             waiver_reason = ?, waiver_approver_id = ? \
             WHERE id = ? AND row_version = ?",
            [
                now.into(),
                waiver_reason.into(),
                waiver_approver_id.into(),
                id.into(),
                expected_rv.into(),
            ],
        ))
        .await?;
    if aff.rows_affected() != 1 {
        return Err(AppError::ValidationFailed(vec![
            "data_integrity_finding was modified elsewhere (stale row_version).".into(),
        ]));
    }
    Ok(())
}

async fn update_finding_repaired(
    txn: &impl ConnectionTrait,
    id: i64,
    expected_rv: i64,
) -> AppResult<()> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let aff = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE data_integrity_findings SET \
             status = 'repaired', cleared_at = ?, row_version = row_version + 1 \
             WHERE id = ? AND row_version = ?",
            [now.into(), id.into(), expected_rv.into()],
        ))
        .await?;
    if aff.rows_affected() != 1 {
        return Err(AppError::ValidationFailed(vec![
            "data_integrity_finding was modified elsewhere (stale row_version).".into(),
        ]));
    }
    Ok(())
}

async fn reload_and_stage_finding(txn: &impl ConnectionTrait, id: i64) -> AppResult<DataIntegrityFindingRow> {
    let f = get_finding(txn, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "data_integrity_findings".into(),
            id: id.to_string(),
        })?;
    stage_finding_sync(
        txn,
        f.id,
        &f.entity_sync_id,
        f.row_version,
        &f.severity,
        &f.domain,
        &f.record_class,
        f.record_id,
        &f.finding_code,
        &f.details_json,
        &f.status,
    )
    .await?;
    Ok(f)
}

async fn insert_repair_audit(
    txn: &impl ConnectionTrait,
    finding_id: i64,
    action: &str,
    actor_id: i64,
    before_json: &str,
    after_json: &str,
) -> AppResult<()> {
    let esid = Uuid::new_v4().to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO data_integrity_repair_actions \
         (entity_sync_id, row_version, finding_id, action, actor_id, performed_at, before_json, after_json) \
         VALUES (?, 1, ?, ?, ?, ?, ?, ?)",
        [
            esid.clone().into(),
            finding_id.into(),
            action.into(),
            actor_id.into(),
            now.into(),
            before_json.into(),
            after_json.into(),
        ],
    ))
    .await?;
    let rid = last_insert_rowid(txn).await?;
    stage_repair_action_sync(
        txn,
        rid,
        &esid,
        1,
        finding_id,
        action,
        actor_id,
        before_json,
        after_json,
    )
    .await
}

pub async fn waive_data_integrity_finding(
    db: &DatabaseConnection,
    input: WaiveDataIntegrityFindingInput,
    actor_id: i64,
) -> AppResult<DataIntegrityFindingRow> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["waiver reason required".into()]));
    }
    let txn = db.begin().await?;
    let f0 = get_finding(&txn, input.finding_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "data_integrity_findings".into(),
            id: input.finding_id.to_string(),
        })?;
    if f0.status != "open" {
        return Err(AppError::ValidationFailed(vec!["finding is not open".into()]));
    }
    if f0.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "data_integrity_finding was modified elsewhere (stale row_version).".into(),
        ]));
    }
    let waiver_approver = if f0.severity == "error" {
        let appr = input.approver_id.ok_or_else(|| {
            AppError::ValidationFailed(vec!["approver_id required for error severity".into()])
        })?;
        if appr == actor_id {
            return Err(AppError::ValidationFailed(vec![
                "second-person approval required for error findings".into(),
            ]));
        }
        appr
    } else {
        input.approver_id.unwrap_or(actor_id)
    };

    let before_json = json!({ "finding": serde_json::to_value(&f0)? }).to_string();

    update_finding_waived(
        &txn,
        input.finding_id,
        input.expected_row_version,
        &input.reason,
        waiver_approver,
    )
    .await?;

    let f1 = reload_and_stage_finding(&txn, input.finding_id).await?;
    let after_json = json!({ "finding": serde_json::to_value(&f1)? }).to_string();

    insert_repair_audit(
        &txn,
        input.finding_id,
        "waive",
        actor_id,
        &before_json,
        &after_json,
    )
    .await?;

    txn.commit().await?;
    get_finding(db, input.finding_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("finding missing after waive")))
}

pub async fn apply_data_integrity_repair(
    db: &DatabaseConnection,
    input: ApplyDataIntegrityRepairInput,
    actor_id: i64,
) -> AppResult<DataIntegrityFindingRow> {
    let txn = db.begin().await?;
    let f0 = get_finding(&txn, input.finding_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "data_integrity_findings".into(),
            id: input.finding_id.to_string(),
        })?;
    if f0.status != "open" {
        return Err(AppError::ValidationFailed(vec!["finding is not open".into()]));
    }
    if f0.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "data_integrity_finding was modified elsewhere (stale row_version).".into(),
        ]));
    }

    let details: Value = serde_json::from_str(&f0.details_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("details_json: {e}")]))?;

    let (before_extra, after_extra, action_label) = match (
        f0.finding_code.as_str(),
        input.repair_kind.as_str(),
    ) {
        ("FK_ORPHAN_FAILURE_MODE", "clear_failure_mode") => {
            let fd_id = f0.record_id;
            let row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, work_order_id, failure_mode_id FROM work_order_failure_details WHERE id = ?",
                    [fd_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_failure_details".into(),
                    id: fd_id.to_string(),
                })?;
            let before_row = json!({
                "id": row.try_get::<i64>("", "id").map_err(|e| map_err("id", e))?,
                "work_order_id": row.try_get::<i64>("", "work_order_id").map_err(|e| map_err("work_order_id", e))?,
                "failure_mode_id": row.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| map_err("failure_mode_id", e))?,
            });
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_failure_details SET failure_mode_id = NULL WHERE id = ?",
                [fd_id.into()],
            ))
            .await?;
            let row2 = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, work_order_id, failure_mode_id FROM work_order_failure_details WHERE id = ?",
                    [fd_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_failure_details".into(),
                    id: fd_id.to_string(),
                })?;
            let after_row = json!({
                "id": row2.try_get::<i64>("", "id").map_err(|e| map_err("id", e))?,
                "work_order_id": row2.try_get::<i64>("", "work_order_id").map_err(|e| map_err("work_order_id", e))?,
                "failure_mode_id": row2.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| map_err("failure_mode_id", e))?,
            });
            (
                json!({ "failure_detail": before_row }),
                json!({ "failure_detail": after_row }),
                "clear_failure_mode",
            )
        }
        ("WO_DOWNTIME_NEGATIVE_DURATION", "swap_downtime_times") => {
            let seg_id = f0.record_id;
            let row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, work_order_id, started_at, ended_at FROM work_order_downtime_segments WHERE id = ?",
                    [seg_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_downtime_segments".into(),
                    id: seg_id.to_string(),
                })?;
            let s: String = row.try_get("", "started_at").map_err(|e| map_err("started_at", e))?;
            let e: String = row.try_get("", "ended_at").map_err(|e| map_err("ended_at", e))?;
            let before_row = json!({ "started_at": s, "ended_at": e });
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_downtime_segments SET started_at = ?, ended_at = ? WHERE id = ?",
                [e.clone().into(), s.clone().into(), seg_id.into()],
            ))
            .await?;
            let after_row = json!({ "started_at": e, "ended_at": s });
            (
                json!({ "segment": before_row }),
                json!({ "segment": after_row }),
                "swap_downtime_times",
            )
        }
        ("WO_DOWNTIME_UNCLOSED", "close_downtime_at_start") => {
            let seg_id = f0.record_id;
            let row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, started_at, ended_at FROM work_order_downtime_segments WHERE id = ?",
                    [seg_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_downtime_segments".into(),
                    id: seg_id.to_string(),
                })?;
            let s: String = row.try_get("", "started_at").map_err(|e| map_err("started_at", e))?;
            let before_row = json!({ "started_at": s, "ended_at": Option::<String>::None });
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_downtime_segments SET ended_at = ? WHERE id = ?",
                [s.clone().into(), seg_id.into()],
            ))
            .await?;
            let after_row = json!({ "started_at": s, "ended_at": s });
            (
                json!({ "segment": before_row }),
                json!({ "segment": after_row }),
                "close_downtime_at_start",
            )
        }
        ("WO_DOWNTIME_OVERLAP", "trim_overlap_end") => {
            let a_id = details
                .get("segment_id_a")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| AppError::ValidationFailed(vec!["segment_id_a missing".into()]))?;
            let b_id = details
                .get("segment_id_b")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| AppError::ValidationFailed(vec!["segment_id_b missing".into()]))?;
            let b_row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT started_at FROM work_order_downtime_segments WHERE id = ?",
                    [b_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_downtime_segments".into(),
                    id: b_id.to_string(),
                })?;
            let b_start: String = b_row.try_get("", "started_at").map_err(|e| map_err("started_at", e))?;
            let a_before = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, started_at, ended_at FROM work_order_downtime_segments WHERE id = ?",
                    [a_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_downtime_segments".into(),
                    id: a_id.to_string(),
                })?;
            let before_row = json!({
                "id": a_id,
                "started_at": a_before.try_get::<String>("", "started_at").map_err(|e| map_err("started_at", e))?,
                "ended_at": a_before.try_get::<Option<String>>("", "ended_at").map_err(|e| map_err("ended_at", e))?,
            });
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_downtime_segments SET ended_at = ? WHERE id = ?",
                [b_start.clone().into(), a_id.into()],
            ))
            .await?;
            let a_after = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id, started_at, ended_at FROM work_order_downtime_segments WHERE id = ?",
                    [a_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "work_order_downtime_segments".into(),
                    id: a_id.to_string(),
                })?;
            let after_row = json!({
                "id": a_id,
                "started_at": a_after.try_get::<String>("", "started_at").map_err(|e| map_err("started_at", e))?,
                "ended_at": a_after.try_get::<Option<String>>("", "ended_at").map_err(|e| map_err("ended_at", e))?,
            });
            (
                json!({ "segment": before_row }),
                json!({ "segment": after_row }),
                "trim_overlap_end",
            )
        }
        ("WO_DOWNTIME_SUM_EXCEEDS_WINDOW", _) => {
            return Err(AppError::ValidationFailed(vec![
                "no automated repair for WO_DOWNTIME_SUM_EXCEEDS_WINDOW: adjust segments or WO window manually"
                    .into(),
            ]));
        }
        _ => {
            return Err(AppError::ValidationFailed(vec![format!(
                "unsupported repair for {} / {}",
                f0.finding_code, input.repair_kind
            )]));
        }
    };

    let before_json = json!({
        "finding": serde_json::to_value(&f0)?,
        "target": before_extra,
    })
    .to_string();

    update_finding_repaired(&txn, input.finding_id, input.expected_row_version).await?;

    let f1 = reload_and_stage_finding(&txn, input.finding_id).await?;
    let after_json = json!({
        "finding": serde_json::to_value(&f1)?,
        "target": after_extra,
    })
    .to_string();

    insert_repair_audit(
        &txn,
        input.finding_id,
        action_label,
        actor_id,
        &before_json,
        &after_json,
    )
    .await?;

    txn.commit().await?;

    get_finding(db, input.finding_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("finding missing after repair")))
}

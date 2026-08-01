use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    InspectionAnomalySyncPayload, InspectionEvidenceSyncPayload, InspectionResultSyncPayload, StageOutboxItemInput,
    SYNC_ENTITY_INSPECTION_ANOMALIES, SYNC_ENTITY_INSPECTION_EVIDENCE, SYNC_ENTITY_INSPECTION_RESULTS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    AddInspectionEvidenceInput, EnqueueInspectionOfflineInput, InspectionAnomaliesFilter, InspectionAnomaly,
    InspectionCheckpoint, InspectionEvidence, InspectionEvidenceFilter, InspectionOfflineQueueItem, InspectionResult,
    InspectionResultsFilter, InspectionRound, RecordInspectionResultInput, UpdateInspectionAnomalyInput,
};
use super::queries::{
    get_inspection_checkpoint_by_id, get_inspection_round_by_id, get_inspection_template_version_by_id,
    stage_inspection_round,
};

const RESULT_STATUSES: &[&str] = &[
    "pass",
    "warning",
    "fail",
    "not_accessible",
    "not_done",
];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inspection_results decode '{field}': {err}"))
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

fn opt_bool_sql(v: Option<bool>) -> sea_orm::Value {
    match v {
        None => sea_orm::Value::from(None::<i64>),
        Some(true) => sea_orm::Value::from(1i64),
        Some(false) => sea_orm::Value::from(0i64),
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct EscalationRules {
    anomaly_on: Vec<String>,
}

impl Default for EscalationRules {
    fn default() -> Self {
        Self {
            anomaly_on: vec!["warning".to_string(), "fail".to_string()],
        }
    }
}

fn parse_escalation(raw: Option<&str>) -> EscalationRules {
    raw.and_then(|s| serde_json::from_str::<EscalationRules>(s).ok())
        .unwrap_or_default()
}

fn escalation_allows(status: &str, rules: &EscalationRules) -> bool {
    rules.anomaly_on.iter().any(|x| x == status)
}

fn validate_result_status(s: &str) -> AppResult<()> {
    if RESULT_STATUSES.contains(&s) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "result_status must be one of {:?}",
            RESULT_STATUSES
        )]))
    }
}

fn eval_numeric_status(v: f64, cp: &InspectionCheckpoint) -> &'static str {
    if let (Some(nmin), Some(nmax)) = (cp.normal_min, cp.normal_max) {
        if v >= nmin && v <= nmax {
            return "pass";
        }
        if let (Some(wmin), Some(wmax)) = (cp.warning_min, cp.warning_max) {
            if v >= wmin && v <= wmax {
                return "warning";
            }
            return "fail";
        }
        return "warning";
    }
    "pass"
}

fn map_result(row: &sea_orm::QueryResult) -> AppResult<InspectionResult> {
    let b: Option<i64> = row.try_get("", "boolean_value").ok();
    Ok(InspectionResult {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        round_id: row.try_get("", "round_id").map_err(|e| decode_err("round_id", e))?,
        checkpoint_id: row
            .try_get("", "checkpoint_id")
            .map_err(|e| decode_err("checkpoint_id", e))?,
        result_status: row
            .try_get("", "result_status")
            .map_err(|e| decode_err("result_status", e))?,
        numeric_value: row.try_get("", "numeric_value").ok(),
        text_value: row.try_get("", "text_value").ok(),
        boolean_value: b.map(|x| x != 0),
        comment: row.try_get("", "comment").ok(),
        recorded_at: row.try_get("", "recorded_at").map_err(|e| decode_err("recorded_at", e))?,
        recorded_by_id: row
            .try_get("", "recorded_by_id")
            .map_err(|e| decode_err("recorded_by_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_evidence(row: &sea_orm::QueryResult) -> AppResult<InspectionEvidence> {
    Ok(InspectionEvidence {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        result_id: row.try_get("", "result_id").map_err(|e| decode_err("result_id", e))?,
        evidence_type: row
            .try_get("", "evidence_type")
            .map_err(|e| decode_err("evidence_type", e))?,
        file_path_or_value: row
            .try_get("", "file_path_or_value")
            .map_err(|e| decode_err("file_path_or_value", e))?,
        captured_at: row.try_get("", "captured_at").map_err(|e| decode_err("captured_at", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub(crate) fn map_anomaly(row: &sea_orm::QueryResult) -> AppResult<InspectionAnomaly> {
    let rpr: i64 = row
        .try_get("", "requires_permit_review")
        .map_err(|e| decode_err("requires_permit_review", e))?;
    Ok(InspectionAnomaly {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        round_id: row.try_get("", "round_id").map_err(|e| decode_err("round_id", e))?,
        result_id: row.try_get("", "result_id").ok(),
        anomaly_type: row
            .try_get("", "anomaly_type")
            .map_err(|e| decode_err("anomaly_type", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        description: row.try_get("", "description").map_err(|e| decode_err("description", e))?,
        linked_di_id: row.try_get("", "linked_di_id").ok(),
        linked_work_order_id: row.try_get("", "linked_work_order_id").ok(),
        requires_permit_review: rpr != 0,
        resolution_status: row
            .try_get("", "resolution_status")
            .map_err(|e| decode_err("resolution_status", e))?,
        routing_decision: row.try_get("", "routing_decision").ok(),
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

async fn count_evidence(db: &DatabaseConnection, result_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM inspection_evidence WHERE result_id = ?",
            [result_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count evidence")))?;
    let c: i64 = row.try_get("", "c").map_err(|e| decode_err("c", e))?;
    Ok(c)
}

async fn stage_inspection_result(db: &DatabaseConnection, row: &InspectionResult) -> AppResult<()> {
    let payload = InspectionResultSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        round_id: row.round_id,
        checkpoint_id: row.checkpoint_id,
        result_status: row.result_status.clone(),
        numeric_value: row.numeric_value,
        text_value: row.text_value.clone(),
        boolean_value: row.boolean_value,
        recorded_at: row.recorded_at.clone(),
        recorded_by_id: row.recorded_by_id,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_results:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_RESULTS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn stage_inspection_evidence(db: &DatabaseConnection, row: &InspectionEvidence) -> AppResult<()> {
    let payload = InspectionEvidenceSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        result_id: row.result_id,
        evidence_type: row.evidence_type.clone(),
        file_path_or_value: row.file_path_or_value.clone(),
        captured_at: row.captured_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_evidence:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_EVIDENCE.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub(crate) async fn stage_inspection_anomaly(db: &DatabaseConnection, row: &InspectionAnomaly) -> AppResult<()> {
    let payload = InspectionAnomalySyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        round_id: row.round_id,
        result_id: row.result_id,
        anomaly_type: row.anomaly_type.clone(),
        severity: row.severity,
        description: row.description.clone(),
        linked_di_id: row.linked_di_id,
        linked_work_order_id: row.linked_work_order_id,
        requires_permit_review: row.requires_permit_review,
        resolution_status: row.resolution_status.clone(),
        routing_decision: row.routing_decision.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("inspection_anomalies:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_INSPECTION_ANOMALIES.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn maybe_bump_round(db: &DatabaseConnection, round: &InspectionRound) -> AppResult<()> {
    if round.status != "scheduled" && round.status != "released" {
        return Ok(());
    }
    let new_rv = round.row_version + 1;
    let aff = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_rounds SET status = 'in_progress', row_version = ? WHERE id = ? AND row_version = ?",
            [new_rv.into(), round.id.into(), round.row_version.into()],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Ok(());
    }
    let updated = get_inspection_round_by_id(db, round.id).await?.expect("row");
    stage_inspection_round(db, &updated).await?;
    Ok(())
}

fn resolve_status(
    cp: &InspectionCheckpoint,
    input: &RecordInspectionResultInput,
) -> AppResult<String> {
    if let Some(ref s) = input.result_status {
        validate_result_status(s)?;
        return Ok(s.clone());
    }
    match cp.check_type.as_str() {
        "numeric" => {
            let Some(v) = input.numeric_value else {
                return Err(AppError::ValidationFailed(vec![
                    "numeric_value required for numeric checkpoints when result_status omitted.".into(),
                ]));
            };
            Ok(eval_numeric_status(v, cp).to_string())
        }
        "pass_fail" | "observation" => Err(AppError::ValidationFailed(vec![
            "result_status is required for this checkpoint type.".into(),
        ])),
        "boolean" => Err(AppError::ValidationFailed(vec![
            "Use boolean_value without result_status for boolean checkpoints.".into(),
        ])),
        _ => Err(AppError::ValidationFailed(vec!["Unknown check_type.".into()])),
    }
}

pub async fn list_inspection_results(
    db: &DatabaseConnection,
    filter: InspectionResultsFilter,
) -> AppResult<Vec<InspectionResult>> {
    let rid = filter.round_id.ok_or_else(|| {
        AppError::ValidationFailed(vec!["round_id is required for listing inspection results.".into()])
    })?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE round_id = ? ORDER BY id ASC",
            [rid.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_result(&r)?);
    }
    Ok(out)
}

pub async fn list_inspection_evidence(
    db: &DatabaseConnection,
    filter: InspectionEvidenceFilter,
) -> AppResult<Vec<InspectionEvidence>> {
    let rid = filter.result_id.ok_or_else(|| {
        AppError::ValidationFailed(vec!["result_id is required for listing inspection evidence.".into()])
    })?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, result_id, evidence_type, file_path_or_value, captured_at, entity_sync_id, row_version \
             FROM inspection_evidence WHERE result_id = ? ORDER BY id ASC",
            [rid.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_evidence(&r)?);
    }
    Ok(out)
}

pub async fn list_inspection_anomalies(
    db: &DatabaseConnection,
    filter: InspectionAnomaliesFilter,
) -> AppResult<Vec<InspectionAnomaly>> {
    let rid = filter.round_id.ok_or_else(|| {
        AppError::ValidationFailed(vec!["round_id is required for listing inspection anomalies.".into()])
    })?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, \
             requires_permit_review, resolution_status, routing_decision, entity_sync_id, row_version \
             FROM inspection_anomalies WHERE round_id = ? ORDER BY id ASC",
            [rid.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(map_anomaly(&r)?);
    }
    Ok(out)
}

pub async fn record_inspection_result(
    db: &DatabaseConnection,
    input: RecordInspectionResultInput,
    recorded_by_id: i64,
) -> AppResult<InspectionResult> {
    let round = get_inspection_round_by_id(db, input.round_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionRound".into(),
            id: input.round_id.to_string(),
        })?;
    let cp = get_inspection_checkpoint_by_id(db, input.checkpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionCheckpoint".into(),
            id: input.checkpoint_id.to_string(),
        })?;
    if cp.template_version_id != round.template_version_id {
        return Err(AppError::ValidationFailed(vec![
            "checkpoint does not belong to this round's template version.".into(),
        ]));
    }

    let status = if cp.check_type == "boolean" {
        if input.result_status.is_some() {
            let s = input.result_status.as_ref().expect("checked");
            validate_result_status(s)?;
            s.clone()
        } else {
            let b = input.boolean_value.ok_or_else(|| {
                AppError::ValidationFailed(vec!["boolean_value required for boolean checkpoints.".into()])
            })?;
            if b {
                "pass".into()
            } else {
                "fail".into()
            }
        }
    } else {
        resolve_status(&cp, &input)?
    };
    validate_result_status(&status)?;

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_results WHERE round_id = ? AND checkpoint_id = ?",
            [input.round_id.into(), input.checkpoint_id.into()],
        ))
        .await?;
    let existing_id: Option<i64> = existing.and_then(|r| r.try_get("", "id").ok());
    let ev_before = if let Some(eid) = existing_id {
        count_evidence(db, eid).await?
    } else {
        0
    };

    if cp.requires_comment_on_exception && status != "pass" {
        let c = input.comment.as_ref().map(|s| s.trim()).unwrap_or("");
        if c.is_empty() && ev_before == 0 {
            return Err(AppError::ValidationFailed(vec![
                "comment or evidence required when requires_comment_on_exception and status is not pass.".into(),
            ]));
        }
    }

    let recorded_at = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    let row = if let Some(eid) = existing_id {
        let cur_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT row_version FROM inspection_results WHERE id = ?",
                [eid.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "InspectionResult".into(),
                id: eid.to_string(),
            })?;
        let cur_rv: i64 = cur_row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?;
        if let Some(exp) = input.expected_row_version {
            if cur_rv != exp {
                return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_results.".into()]));
            }
        }
        let new_rv = cur_rv + 1;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_results SET result_status = ?, numeric_value = ?, text_value = ?, boolean_value = ?, \
             comment = ?, recorded_at = ?, recorded_by_id = ?, row_version = ? WHERE id = ?",
            [
                status.clone().into(),
                opt_f64(input.numeric_value),
                opt_string(input.text_value.clone()),
                opt_bool_sql(input.boolean_value),
                opt_string(input.comment.clone()),
                recorded_at.clone().into(),
                recorded_by_id.into(),
                new_rv.into(),
                eid.into(),
            ],
        ))
        .await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [eid.into()],
        ))
        .await?
        .expect("row")
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_results (entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, \
             text_value, boolean_value, comment, recorded_at, recorded_by_id, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                sync_id.into(),
                input.round_id.into(),
                input.checkpoint_id.into(),
                status.clone().into(),
                opt_f64(input.numeric_value),
                opt_string(input.text_value.clone()),
                opt_bool_sql(input.boolean_value),
                opt_string(input.comment.clone()),
                recorded_at.into(),
                recorded_by_id.into(),
            ],
        ))
        .await?;
        let id_row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let nid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .expect("row")
    };

    let result = map_result(&row)?;
    stage_inspection_result(db, &result).await?;

    let ev_after = count_evidence(db, result.id).await?;
    let version = get_inspection_template_version_by_id(db, round.template_version_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("template version")))?;
    let esc = parse_escalation(version.escalation_rules_json.as_deref());

    if cp.requires_photo && ev_after == 0 {
        insert_auto_anomaly(
            db,
            round.id,
            Some(result.id),
            "missing_mandatory_photo",
            3,
            "Mandatory photo not attached for this checkpoint.",
        )
        .await?;
    }

    if (status == "warning" || status == "fail") && escalation_allows(&status, &esc) {
        insert_auto_anomaly(
            db,
            round.id,
            Some(result.id),
            "threshold_breach",
            if status == "fail" { 4 } else { 2 },
            "Result outside normal or warning tolerance bands.",
        )
        .await?;
    }

    let round_refresh = get_inspection_round_by_id(db, round.id).await?.expect("row");
    maybe_bump_round(db, &round_refresh).await?;

    get_inspection_result_by_id(db, result.id).await?.ok_or_else(|| AppError::Internal(anyhow::anyhow!("result")))
}

async fn insert_auto_anomaly(
    db: &DatabaseConnection,
    round_id: i64,
    result_id: Option<i64>,
    anomaly_type: &str,
    severity: i64,
    description: &str,
) -> AppResult<()> {
    let Some(rid) = result_id else {
        return Ok(());
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM inspection_anomalies WHERE result_id = ? AND anomaly_type = ?",
        [rid.into(), anomaly_type.into()],
    ))
    .await?;
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_anomalies (entity_sync_id, round_id, result_id, anomaly_type, severity, description, \
         linked_di_id, linked_work_order_id, requires_permit_review, resolution_status, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, 0, 'open', 1)",
        [
            sync_id.into(),
            round_id.into(),
            rid.into(),
            anomaly_type.into(),
            severity.into(),
            description.into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let aid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let arow = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, \
             requires_permit_review, resolution_status, routing_decision, entity_sync_id, row_version FROM inspection_anomalies WHERE id = ?",
            [aid.into()],
        ))
        .await?
        .expect("row");
    let an = map_anomaly(&arow)?;
    stage_inspection_anomaly(db, &an).await?;
    Ok(())
}

async fn get_inspection_result_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<InspectionResult>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|r| map_result(&r)).transpose()?)
}

pub async fn add_inspection_evidence(
    db: &DatabaseConnection,
    input: AddInspectionEvidenceInput,
    _actor_personnel_id: i64,
) -> AppResult<InspectionEvidence> {
    if input.evidence_type.trim().is_empty() || input.file_path_or_value.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["evidence_type and file_path_or_value required.".into()]));
    }
    let res = get_inspection_result_by_id(db, input.result_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionResult".into(),
            id: input.result_id.to_string(),
        })?;
    if let Some(exp) = input.expected_row_version {
        if res.row_version != exp {
            return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_results.".into()]));
        }
    }
    let cap = input.captured_at.unwrap_or_else(|| Utc::now().to_rfc3339());
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_evidence (result_id, evidence_type, file_path_or_value, captured_at, entity_sync_id, \
         row_version) VALUES (?, ?, ?, ?, ?, 1)",
        [
            input.result_id.into(),
            input.evidence_type.trim().into(),
            input.file_path_or_value.into(),
            cap.into(),
            sync_id.into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let eid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, result_id, evidence_type, file_path_or_value, captured_at, entity_sync_id, row_version \
             FROM inspection_evidence WHERE id = ?",
            [eid.into()],
        ))
        .await?
        .expect("row");
    let ev = map_evidence(&row)?;
    stage_inspection_evidence(db, &ev).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM inspection_anomalies WHERE result_id = ? AND anomaly_type = 'missing_mandatory_photo'",
        [res.id.into()],
    ))
    .await?;

    Ok(ev)
}

pub async fn update_inspection_anomaly(
    db: &DatabaseConnection,
    input: UpdateInspectionAnomalyInput,
) -> AppResult<InspectionAnomaly> {
    let cur = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM inspection_anomalies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionAnomaly".into(),
            id: input.id.to_string(),
        })?;
    let rv: i64 = cur.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?;
    if rv != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_anomalies.".into()]));
    }
    let new_rv = rv + 1;
    let rpr = input.requires_permit_review.unwrap_or(false);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_anomalies SET resolution_status = ?, linked_di_id = ?, linked_work_order_id = ?, \
         requires_permit_review = ?, row_version = ? WHERE id = ?",
        [
            input.resolution_status.into(),
            opt_i64(input.linked_di_id),
            opt_i64(input.linked_work_order_id),
            (if rpr { 1 } else { 0 }).into(),
            new_rv.into(),
            input.id.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, \
             requires_permit_review, resolution_status, routing_decision, entity_sync_id, row_version FROM inspection_anomalies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .expect("row");
    let an = map_anomaly(&row)?;
    stage_inspection_anomaly(db, &an).await?;
    Ok(an)
}

pub async fn enqueue_inspection_offline(db: &DatabaseConnection, input: EnqueueInspectionOfflineInput) -> AppResult<i64> {
    if input.local_temp_id.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["local_temp_id required.".into()]));
    }
    serde_json::from_str::<serde_json::Value>(&input.payload_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("payload_json: {e}")]))?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_offline_queue (payload_json, local_temp_id, sync_status) VALUES (?, ?, 'pending')",
        [input.payload_json.into(), input.local_temp_id.into()],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    Ok(id_row.try_get("", "id").map_err(|e| decode_err("id", e))?)
}

pub async fn list_inspection_offline_queue(db: &DatabaseConnection) -> AppResult<Vec<InspectionOfflineQueueItem>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, payload_json, local_temp_id, sync_status FROM inspection_offline_queue ORDER BY id ASC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(InspectionOfflineQueueItem {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            payload_json: r.try_get("", "payload_json").map_err(|e| decode_err("payload_json", e))?,
            local_temp_id: r.try_get("", "local_temp_id").map_err(|e| decode_err("local_temp_id", e))?,
            sync_status: r.try_get("", "sync_status").map_err(|e| decode_err("sync_status", e))?,
        });
    }
    Ok(out)
}

pub async fn mark_inspection_offline_synced(db: &DatabaseConnection, queue_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_offline_queue SET sync_status = 'synced' WHERE id = ?",
        [queue_id.into()],
    ))
    .await?;
    Ok(())
}

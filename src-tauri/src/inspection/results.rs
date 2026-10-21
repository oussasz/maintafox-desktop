use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};

use super::domain::{
    AddInspectionEvidenceInput, InspectionAnomaliesFilter, InspectionAnomaly, InspectionCheckpoint, InspectionEvidence,
    InspectionEvidenceFilter, InspectionResult, InspectionResultsFilter, InspectionRound, RecordInspectionResultInput,
    UpdateInspectionAnomalyInput,
};
use super::queries::{
    get_inspection_checkpoint_by_id, get_inspection_round_by_id, get_inspection_template_version_by_id,
};

pub(crate) const RESULT_STATUSES: &[&str] = &["pass", "warning", "fail", "not_accessible", "not_done"];

/// Results that are not a clean pass (they need a follow-up decision or an explanation).
pub(crate) const EXCEPTION_STATUSES: &[&str] = &["warning", "fail", "not_accessible", "not_done"];

/// Results that say the check could not be performed: a reason is always required.
const NOT_PERFORMED_STATUSES: &[&str] = &["not_accessible", "not_done"];

/// Round statuses in which results may still be recorded.
pub(crate) const ROUND_OPEN_STATUSES: &[&str] = &["scheduled", "released", "in_progress"];

pub(crate) const ANOMALY_STATUSES: &[&str] = &["open", "triaged", "deferred", "resolved", "cancelled"];

pub(crate) const ANOMALY_THRESHOLD_BREACH: &str = "threshold_breach";
pub(crate) const ANOMALY_MISSING_PHOTO: &str = "missing_mandatory_photo";

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
#[serde(default, deny_unknown_fields)]
pub(crate) struct EscalationRules {
    anomaly_on: Vec<String>,
}

impl Default for EscalationRules {
    fn default() -> Self {
        Self {
            anomaly_on: vec!["warning".to_string(), "fail".to_string()],
        }
    }
}

/// Escalation rules of a template version. No rules: anomalies on `warning` and `fail` (the
/// documented default of PRD §6.25). Malformed stored rules are an error, never a silent default.
pub(crate) fn parse_escalation(raw: Option<&str>) -> AppResult<EscalationRules> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(EscalationRules::default());
    };
    let rules: EscalationRules = serde_json::from_str(raw)
        .map_err(|e| AppError::ValidationFailed(vec![format!("escalation_rules_json is invalid: {e}")]))?;
    if let Some(bad) = rules.anomaly_on.iter().find(|s| !RESULT_STATUSES.contains(&s.as_str())) {
        return Err(AppError::ValidationFailed(vec![format!(
            "escalation_rules_json: unknown result status '{bad}'."
        )]));
    }
    Ok(rules)
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

fn within(v: f64, lo: Option<f64>, hi: Option<f64>) -> bool {
    lo.is_none_or(|l| v >= l) && hi.is_none_or(|h| v <= h)
}

/// Classifies a reading against the checkpoint limits. Each bound is optional (one-sided limits
/// such as "temperature ≤ 80 °C" are common). No limit at all: a plain measurement, `pass`.
/// Outside the normal band: `warning` inside the warning band, `fail` beyond it; when no warning
/// band is defined, outside the normal band is a `warning`.
pub(crate) fn eval_numeric_status(v: f64, cp: &InspectionCheckpoint) -> &'static str {
    let has_normal = cp.normal_min.is_some() || cp.normal_max.is_some();
    let has_warning = cp.warning_min.is_some() || cp.warning_max.is_some();
    if !has_normal && !has_warning {
        return "pass";
    }
    if !has_normal {
        // Only a warning band: inside it is acceptable, beyond it fails.
        return if within(v, cp.warning_min, cp.warning_max) {
            "pass"
        } else {
            "fail"
        };
    }
    if within(v, cp.normal_min, cp.normal_max) {
        return "pass";
    }
    if !has_warning {
        return "warning";
    }
    if within(v, cp.warning_min, cp.warning_max) {
        "warning"
    } else {
        "fail"
    }
}

/// A measurement outside its limits is always explained by the technician: the comment says
/// what was seen and travels with the follow-up request.
pub(crate) fn reading_needs_comment(cp: &InspectionCheckpoint, status: &str) -> bool {
    cp.check_type == "numeric" && (status == "warning" || status == "fail")
}

/// The limit a reading crossed: `("above", max)` or `("below", min)`. Beyond the warning band
/// the warning limit is reported, otherwise the normal limit. `None` inside the limits.
pub(crate) fn violated_limit(v: f64, cp: &InspectionCheckpoint) -> Option<(&'static str, f64)> {
    let beyond = |lo: Option<f64>, hi: Option<f64>| -> Option<(&'static str, f64)> {
        if let Some(h) = hi.filter(|h| v > *h) {
            return Some(("above", h));
        }
        lo.filter(|l| v < *l).map(|l| ("below", l))
    };
    beyond(cp.warning_min, cp.warning_max).or_else(|| beyond(cp.normal_min, cp.normal_max))
}

fn map_result(row: &sea_orm::QueryResult) -> AppResult<InspectionResult> {
    let b: Option<i64> = row
        .try_get("", "boolean_value")
        .map_err(|e| decode_err("boolean_value", e))?;
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
        numeric_value: row
            .try_get("", "numeric_value")
            .map_err(|e| decode_err("numeric_value", e))?,
        text_value: row.try_get("", "text_value").map_err(|e| decode_err("text_value", e))?,
        boolean_value: b.map(|x| x != 0),
        comment: row.try_get("", "comment").map_err(|e| decode_err("comment", e))?,
        recorded_at: row
            .try_get("", "recorded_at")
            .map_err(|e| decode_err("recorded_at", e))?,
        recorded_by_id: row
            .try_get("", "recorded_by_id")
            .map_err(|e| decode_err("recorded_by_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
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
        captured_at: row
            .try_get("", "captured_at")
            .map_err(|e| decode_err("captured_at", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

pub(crate) fn map_anomaly(row: &sea_orm::QueryResult) -> AppResult<InspectionAnomaly> {
    let rpr: i64 = row
        .try_get("", "requires_permit_review")
        .map_err(|e| decode_err("requires_permit_review", e))?;
    Ok(InspectionAnomaly {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        round_id: row.try_get("", "round_id").map_err(|e| decode_err("round_id", e))?,
        result_id: row.try_get("", "result_id").map_err(|e| decode_err("result_id", e))?,
        anomaly_type: row
            .try_get("", "anomaly_type")
            .map_err(|e| decode_err("anomaly_type", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        description: row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?,
        linked_di_id: row
            .try_get("", "linked_di_id")
            .map_err(|e| decode_err("linked_di_id", e))?,
        linked_work_order_id: row
            .try_get("", "linked_work_order_id")
            .map_err(|e| decode_err("linked_work_order_id", e))?,
        requires_permit_review: rpr != 0,
        resolution_status: row
            .try_get("", "resolution_status")
            .map_err(|e| decode_err("resolution_status", e))?,
        routing_decision: row
            .try_get("", "routing_decision")
            .map_err(|e| decode_err("routing_decision", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

async fn count_evidence(db: &impl ConnectionTrait, result_id: i64) -> AppResult<i64> {
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

/// Results can only be recorded on an open round; the first result starts a scheduled round.
async fn open_round_for_results(db: &impl ConnectionTrait, round: &InspectionRound) -> AppResult<()> {
    if !ROUND_OPEN_STATUSES.contains(&round.status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "The round is '{}': results can no longer be recorded.",
            round.status
        )]));
    }
    if round.status == "in_progress" {
        return Ok(());
    }
    let aff = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_rounds SET status = 'in_progress', started_at = COALESCE(started_at, ?), \
             row_version = row_version + 1 WHERE id = ? AND row_version = ?",
            [
                Utc::now().to_rfc3339().into(),
                round.id.into(),
                round.row_version.into(),
            ],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on inspection_rounds.".into(),
        ]));
    }
    Ok(())
}

fn resolve_status(cp: &InspectionCheckpoint, input: &RecordInspectionResultInput) -> AppResult<String> {
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
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
    db: &impl ConnectionTrait,
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
    let txn = db.begin().await?;
    let round = get_inspection_round_by_id(&txn, input.round_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionRound".into(),
            id: input.round_id.to_string(),
        })?;
    let cp = get_inspection_checkpoint_by_id(&txn, input.checkpoint_id)
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
    open_round_for_results(&txn, &round).await?;

    let status = if cp.check_type == "boolean" {
        if let Some(s) = input.result_status.as_ref() {
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

    let existing = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_results WHERE round_id = ? AND checkpoint_id = ?",
            [input.round_id.into(), input.checkpoint_id.into()],
        ))
        .await?;
    let existing_id: Option<i64> = existing
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(|e| decode_err("id", e))?;
    let ev_before = if let Some(eid) = existing_id {
        count_evidence(&txn, eid).await?
    } else {
        0
    };

    let comment_given = input.comment.as_deref().is_some_and(|c| !c.trim().is_empty());
    if NOT_PERFORMED_STATUSES.contains(&status.as_str()) && !comment_given {
        return Err(AppError::ValidationFailed(vec![
            "A reason (comment) is required when a checkpoint is not accessible or not done.".into(),
        ]));
    }
    if cp.requires_comment_on_exception && status != "pass" {
        let c = input.comment.as_ref().map(|s| s.trim()).unwrap_or("");
        if c.is_empty() && ev_before == 0 {
            return Err(AppError::ValidationFailed(vec![
                "comment or evidence required when requires_comment_on_exception and status is not pass.".into(),
            ]));
        }
    }
    if reading_needs_comment(&cp, &status) && !comment_given {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_READING_COMMENT_REQUIRED: {}: a reading outside its limits needs a comment.",
            cp.checkpoint_code
        )]));
    }

    let recorded_at = Utc::now().to_rfc3339();
    let entity_sync_id = Uuid::new_v4().to_string();

    let row = if let Some(eid) = existing_id {
        let cur_row = txn
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
        let cur_rv: i64 = cur_row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?;
        if let Some(exp) = input.expected_row_version {
            if cur_rv != exp {
                return Err(AppError::ValidationFailed(vec![
                    "row_version mismatch on inspection_results.".into(),
                ]));
            }
        }
        let new_rv = cur_rv + 1;
        txn.execute(Statement::from_sql_and_values(
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
        txn.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [eid.into()],
        ))
        .await?
        .expect("row")
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_results (entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, \
             text_value, boolean_value, comment, recorded_at, recorded_by_id, row_version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                entity_sync_id.into(),
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
        let id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let nid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        txn.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .expect("row")
    };

    let result = map_result(&row)?;

    let ev_after = count_evidence(&txn, result.id).await?;
    let version = get_inspection_template_version_by_id(&txn, round.template_version_id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("template version")))?;
    let esc = parse_escalation(version.escalation_rules_json.as_deref())?;

    sync_auto_anomaly(
        &txn,
        round.id,
        result.id,
        ANOMALY_MISSING_PHOTO,
        cp.requires_photo && ev_after == 0,
        3,
        "Mandatory photo not attached for this checkpoint.",
    )
    .await?;
    sync_auto_anomaly(
        &txn,
        round.id,
        result.id,
        ANOMALY_THRESHOLD_BREACH,
        (status == "warning" || status == "fail") && escalation_allows(&status, &esc),
        if status == "fail" { 4 } else { 2 },
        "Result outside normal or warning tolerance bands.",
    )
    .await?;

    let out = get_inspection_result_by_id(&txn, result.id)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("result")))?;
    txn.commit().await?;
    Ok(out)
}

/// Keeps the automatic anomaly of one kind in line with the latest reading of a result, without
/// ever touching an anomaly someone already acted on (routed to a DI/WO, deferred, resolved):
/// - condition present: refresh the open, unrouted anomaly, or create one if none is pending;
/// - condition gone (reading corrected, photo added): cancel the open, unrouted anomaly.
pub(crate) async fn sync_auto_anomaly(
    db: &impl ConnectionTrait,
    round_id: i64,
    result_id: i64,
    anomaly_type: &str,
    active: bool,
    severity: i64,
    description: &str,
) -> AppResult<()> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, resolution_status, routing_decision, linked_di_id, linked_work_order_id \
             FROM inspection_anomalies WHERE result_id = ? AND anomaly_type = ?",
            [result_id.into(), anomaly_type.into()],
        ))
        .await?;
    let mut open_unrouted: Vec<i64> = Vec::new();
    let mut acted_on = false;
    for r in &rows {
        let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let status: String = r
            .try_get("", "resolution_status")
            .map_err(|e| decode_err("resolution_status", e))?;
        let routing: Option<String> = r
            .try_get("", "routing_decision")
            .map_err(|e| decode_err("routing_decision", e))?;
        let di: Option<i64> = r
            .try_get("", "linked_di_id")
            .map_err(|e| decode_err("linked_di_id", e))?;
        let wo: Option<i64> = r
            .try_get("", "linked_work_order_id")
            .map_err(|e| decode_err("linked_work_order_id", e))?;
        if status == "open" && routing.is_none() && di.is_none() && wo.is_none() {
            open_unrouted.push(id);
        } else if status != "cancelled" {
            acted_on = true;
        }
    }
    if active {
        if let Some(id) = open_unrouted.first() {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inspection_anomalies SET severity = ?, description = ?, row_version = row_version + 1 \
                 WHERE id = ? AND (severity <> ? OR description <> ?)",
                [
                    severity.into(),
                    description.into(),
                    (*id).into(),
                    severity.into(),
                    description.into(),
                ],
            ))
            .await?;
        } else if !acted_on {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO inspection_anomalies (entity_sync_id, round_id, result_id, anomaly_type, severity, \
                 description, linked_di_id, linked_work_order_id, requires_permit_review, resolution_status, \
                 row_version) VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, 0, 'open', 1)",
                [
                    Uuid::new_v4().to_string().into(),
                    round_id.into(),
                    result_id.into(),
                    anomaly_type.into(),
                    severity.into(),
                    description.into(),
                ],
            ))
            .await?;
        }
    } else {
        for id in open_unrouted {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inspection_anomalies SET resolution_status = 'cancelled', row_version = row_version + 1 \
                 WHERE id = ?",
                [id.into()],
            ))
            .await?;
        }
    }
    Ok(())
}

async fn get_inspection_result_by_id(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<InspectionResult>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, round_id, checkpoint_id, result_status, numeric_value, text_value, boolean_value, \
             comment, recorded_at, recorded_by_id, row_version FROM inspection_results WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_result(&r)).transpose()
}

pub async fn add_inspection_evidence(
    db: &DatabaseConnection,
    input: AddInspectionEvidenceInput,
    _actor_personnel_id: i64,
) -> AppResult<InspectionEvidence> {
    if input.evidence_type.trim().is_empty() || input.file_path_or_value.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "evidence_type and file_path_or_value required.".into(),
        ]));
    }
    let res = get_inspection_result_by_id(db, input.result_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionResult".into(),
            id: input.result_id.to_string(),
        })?;
    if let Some(exp) = input.expected_row_version {
        if res.row_version != exp {
            return Err(AppError::ValidationFailed(vec![
                "row_version mismatch on inspection_results.".into(),
            ]));
        }
    }
    let cap = input.captured_at.unwrap_or_else(|| Utc::now().to_rfc3339());
    let txn = db.begin().await?;
    let entity_sync_id = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_evidence (result_id, evidence_type, file_path_or_value, captured_at, entity_sync_id, \
         row_version) VALUES (?, ?, ?, ?, ?, 1)",
        [
            input.result_id.into(),
            input.evidence_type.trim().into(),
            input.file_path_or_value.into(),
            cap.into(),
            entity_sync_id.into(),
        ],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let eid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, result_id, evidence_type, file_path_or_value, captured_at, entity_sync_id, row_version \
             FROM inspection_evidence WHERE id = ?",
            [eid.into()],
        ))
        .await?
        .expect("row");
    let ev = map_evidence(&row)?;

    if ev.evidence_type == "photo" {
        let round = res.round_id;
        sync_auto_anomaly(&txn, round, res.id, ANOMALY_MISSING_PHOTO, false, 3, "").await?;
    }

    txn.commit().await?;
    Ok(ev)
}

pub async fn update_inspection_anomaly(
    db: &DatabaseConnection,
    input: UpdateInspectionAnomalyInput,
) -> AppResult<InspectionAnomaly> {
    if !ANOMALY_STATUSES.contains(&input.resolution_status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "resolution_status must be one of {ANOMALY_STATUSES:?}"
        )]));
    }
    let txn = db.begin().await?;
    let cur = txn
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
    let rv: i64 = cur
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if rv != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on inspection_anomalies.".into(),
        ]));
    }
    let new_rv = rv + 1;
    let rpr = input.requires_permit_review.unwrap_or(false);
    txn.execute(Statement::from_sql_and_values(
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
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, \
             requires_permit_review, resolution_status, routing_decision, entity_sync_id, row_version FROM inspection_anomalies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .expect("row");
    let an = map_anomaly(&row)?;
    txn.commit().await?;
    Ok(an)
}

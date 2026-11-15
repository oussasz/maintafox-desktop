//! Field runner for inspection rounds (PRD §6.25, Phase 6.2).
//!
//! Everything the technician's screen needs, with labels resolved (template, asset, people) so
//! the UI never shows a raw id, plus the per-row sync state so the offline queue is visible:
//! a result saved on this device stays "waiting" until the server has accepted it.
//!
//! Round lifecycle handled here: `scheduled|released → in_progress` (start, or the first
//! result), `in_progress → completed | completed_with_findings` (complete, every checkpoint
//! answered and its evidence rules met), `completed_with_findings → reviewed` (a supervisor
//! review once every anomaly has a decision). A version with `requires_review` also needs the
//! review after a clean `completed`.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::row_state::{row_sync_states, ROW_SYNC_SYNCED};

use super::domain::{
    AddInspectionPhotoInput, InspectionAnomaliesFilter, InspectionCheckpoint, InspectionCheckpointHistoryPoint,
    InspectionCheckpointsFilter, InspectionEvidence, InspectionEvidenceFilter, InspectionResult,
    InspectionResultsFilter, InspectionRound, InspectionRoundActionInput, InspectionRoundPage, InspectionRoundRunner,
    InspectionRoundSummary, InspectionRoundSummaryFilter, InspectionRunnerCheckpoint,
};
use super::queries::{get_inspection_round_by_id, ROUND_COLUMNS};
use super::results::{
    reading_needs_comment, sync_auto_anomaly, ANOMALY_MISSING_PHOTO, EXCEPTION_STATUSES, ROUND_OPEN_STATUSES,
};

/// Missing-item codes shown next to a checkpoint (translated in the UI).
pub const MISSING_RESULT: &str = "result";
pub const MISSING_PHOTO: &str = "photo";
pub const MISSING_COMMENT: &str = "comment";

/// Largest photo accepted as inspection evidence.
pub const MAX_PHOTO_BYTES: usize = 15 * 1024 * 1024;
const PHOTO_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp", "image/heic", "image/heif"];

pub const ROUND_STATUSES: &[&str] = &[
    "scheduled",
    "released",
    "in_progress",
    "completed",
    "completed_with_findings",
    "reviewed",
    "missed",
    "cancelled",
];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inspection runner decode '{field}': {err}"))
}

/// Rows saved here and not yet accepted by the server. Same rule as [`row_sync_states`],
/// written as SQL for list views.
fn pending_rows_sql(entity_type: &str, alias: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM sync_capture_journal j WHERE j.entity_type = '{entity_type}' \
                 AND j.entity_sync_id = {alias}.entity_sync_id) \
          OR EXISTS (SELECT 1 FROM sync_outbox o WHERE o.entity_type = '{entity_type}' \
                 AND o.entity_sync_id = {alias}.entity_sync_id AND o.status IN ('pending', 'in_flight')))"
    )
}

fn summary_sql(where_clause: &str) -> String {
    let round_cols = ROUND_COLUMNS
        .split(',')
        .map(|c| format!("ir.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {round_cols}, \
         t.code AS template_code, t.name AS template_name, v.version_no, v.requires_review, \
         p.full_name AS assigned_to_name, \
         tm.name AS assigned_team_name, \
         sch.code AS schedule_code, sch.name AS schedule_name, \
         (CASE WHEN ir.status IN ('scheduled', 'released', 'in_progress') AND ir.scheduled_at IS NOT NULL \
               AND julianday(ir.scheduled_at) < julianday('now') THEN 1 ELSE 0 END) AS is_overdue, \
         COALESCE(ua.display_name, ua.username) AS reviewed_by_name, \
         (SELECT COUNT(*) FROM inspection_checkpoints c WHERE c.template_version_id = ir.template_version_id) \
             AS checkpoint_count, \
         (SELECT COUNT(*) FROM inspection_results r WHERE r.round_id = ir.id) AS recorded_count, \
         (SELECT COUNT(*) FROM inspection_results r WHERE r.round_id = ir.id \
             AND r.result_status IN ('warning', 'fail', 'not_accessible', 'not_done')) AS exception_count, \
         (SELECT COUNT(*) FROM inspection_anomalies a WHERE a.round_id = ir.id AND a.resolution_status = 'open') \
             AS open_anomaly_count, \
         ((CASE WHEN {round_pending} THEN 1 ELSE 0 END) \
          + (SELECT COUNT(*) FROM inspection_results r WHERE r.round_id = ir.id AND {result_pending}) \
          + (SELECT COUNT(*) FROM inspection_evidence e JOIN inspection_results r2 ON r2.id = e.result_id \
                WHERE r2.round_id = ir.id AND {evidence_pending}) \
          + (SELECT COUNT(*) FROM inspection_anomalies a2 WHERE a2.round_id = ir.id AND {anomaly_pending})) \
             AS pending_sync_count \
         FROM inspection_rounds ir \
         JOIN inspection_templates t ON t.id = ir.template_id \
         JOIN inspection_template_versions v ON v.id = ir.template_version_id \
         LEFT JOIN personnel p ON p.id = ir.assigned_to_id \
         LEFT JOIN teams tm ON tm.id = ir.assigned_team_id \
         LEFT JOIN inspection_schedules sch ON sch.id = ir.schedule_id \
         LEFT JOIN user_accounts ua ON ua.id = ir.reviewed_by_id \
         {where_clause}",
        round_pending = pending_rows_sql("inspection_rounds", "ir"),
        result_pending = pending_rows_sql("inspection_results", "r"),
        evidence_pending = pending_rows_sql("inspection_evidence", "e"),
        anomaly_pending = pending_rows_sql("inspection_anomalies", "a2"),
    )
}

fn map_summary(row: &sea_orm::QueryResult) -> AppResult<InspectionRoundSummary> {
    let round = super::queries::map_round(row)?;
    Ok(InspectionRoundSummary {
        round,
        template_code: row
            .try_get("", "template_code")
            .map_err(|e| decode_err("template_code", e))?,
        template_name: row
            .try_get("", "template_name")
            .map_err(|e| decode_err("template_name", e))?,
        version_no: row.try_get("", "version_no").map_err(|e| decode_err("version_no", e))?,
        requires_review: row
            .try_get::<i64>("", "requires_review")
            .map_err(|e| decode_err("requires_review", e))?
            != 0,
        assigned_to_name: row
            .try_get("", "assigned_to_name")
            .map_err(|e| decode_err("assigned_to_name", e))?,
        assigned_team_name: row
            .try_get("", "assigned_team_name")
            .map_err(|e| decode_err("assigned_team_name", e))?,
        schedule_code: row
            .try_get("", "schedule_code")
            .map_err(|e| decode_err("schedule_code", e))?,
        schedule_name: row
            .try_get("", "schedule_name")
            .map_err(|e| decode_err("schedule_name", e))?,
        is_overdue: row
            .try_get::<i64>("", "is_overdue")
            .map_err(|e| decode_err("is_overdue", e))?
            != 0,
        reviewed_by_name: row
            .try_get("", "reviewed_by_name")
            .map_err(|e| decode_err("reviewed_by_name", e))?,
        checkpoint_count: row
            .try_get("", "checkpoint_count")
            .map_err(|e| decode_err("checkpoint_count", e))?,
        recorded_count: row
            .try_get("", "recorded_count")
            .map_err(|e| decode_err("recorded_count", e))?,
        exception_count: row
            .try_get("", "exception_count")
            .map_err(|e| decode_err("exception_count", e))?,
        open_anomaly_count: row
            .try_get("", "open_anomaly_count")
            .map_err(|e| decode_err("open_anomaly_count", e))?,
        pending_sync_count: row
            .try_get("", "pending_sync_count")
            .map_err(|e| decode_err("pending_sync_count", e))?,
    })
}

/// Largest page of rounds returned at once.
pub const MAX_ROUND_PAGE: i64 = 1000;
/// Page size when the caller does not give one.
const DEFAULT_ROUND_PAGE: i64 = 200;

fn parse_instant(raw: &str, field: &str) -> AppResult<String> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|d| d.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_INSP_DATE_INVALID: {field} must be an RFC 3339 timestamp."
            )])
        })
}

/// `WHERE` clause and binds of a round filter (shared by the page and its count).
fn round_filter_sql(filter: &InspectionRoundSummaryFilter) -> AppResult<(String, Vec<sea_orm::Value>)> {
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(statuses) = filter.statuses.as_ref().filter(|s| !s.is_empty()) {
        if let Some(bad) = statuses.iter().find(|s| !ROUND_STATUSES.contains(&s.as_str())) {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_INSP_ROUND_STATUS_INVALID: Unknown round status '{bad}'."
            )]));
        }
        clauses.push(format!("ir.status IN ({})", vec!["?"; statuses.len()].join(", ")));
        values.extend(statuses.iter().cloned().map(sea_orm::Value::from));
    }
    if filter.overdue_only == Some(true) {
        clauses.push(super::insights::ROUND_OVERDUE_SQL.to_string());
    }
    if let Some(from) = filter.due_from.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push("julianday(ir.scheduled_at) >= julianday(?)".to_string());
        values.push(parse_instant(from, "due_from")?.into());
    }
    if let Some(to) = filter.due_to.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push("julianday(ir.scheduled_at) <= julianday(?)".to_string());
        values.push(parse_instant(to, "due_to")?.into());
    }
    if let Some(schedule_id) = filter.schedule_id {
        clauses.push("ir.schedule_id = ?".to_string());
        values.push(schedule_id.into());
    }
    if let Some(search) = filter.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let pattern = format!("%{search}%");
        clauses.push(
            "(t.name LIKE ? OR t.code LIKE ? OR p.full_name LIKE ? OR tm.name LIKE ? OR sch.code LIKE ? \
             OR sch.name LIKE ?)"
                .to_string(),
        );
        values.extend(std::iter::repeat_n(sea_orm::Value::from(pattern), 6));
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Ok((where_clause, values))
}

/// Rounds for the round list, newest scheduled first, filtered by status, planned period,
/// schedule or overdue state.
pub async fn list_inspection_round_summaries(
    db: &impl ConnectionTrait,
    filter: InspectionRoundSummaryFilter,
) -> AppResult<Vec<InspectionRoundSummary>> {
    let limit = filter.limit.unwrap_or(DEFAULT_ROUND_PAGE).clamp(1, MAX_ROUND_PAGE);
    let offset = filter.offset.unwrap_or(0).max(0);
    let (where_clause, mut values) = round_filter_sql(&filter)?;
    values.push(limit.into());
    values.push(offset.into());
    let sql = format!(
        "{} ORDER BY COALESCE(ir.scheduled_at, '') DESC, ir.id DESC LIMIT ? OFFSET ?",
        summary_sql(&where_clause)
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_summary).collect()
}

/// A page of rounds with the number of rounds matching the filter (the list says when it shows
/// only part of them).
pub async fn list_inspection_round_page(
    db: &impl ConnectionTrait,
    filter: InspectionRoundSummaryFilter,
) -> AppResult<InspectionRoundPage> {
    let (where_clause, values) = round_filter_sql(&filter)?;
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS c FROM inspection_rounds ir \
                 JOIN inspection_templates t ON t.id = ir.template_id \
                 LEFT JOIN personnel p ON p.id = ir.assigned_to_id \
                 LEFT JOIN teams tm ON tm.id = ir.assigned_team_id \
                 LEFT JOIN inspection_schedules sch ON sch.id = ir.schedule_id \
                 {where_clause}"
            ),
            values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("round count")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    let items = list_inspection_round_summaries(db, filter).await?;
    Ok(InspectionRoundPage { items, total })
}

async fn get_round_summary(db: &impl ConnectionTrait, round_id: i64) -> AppResult<InspectionRoundSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            summary_sql("WHERE ir.id = ?"),
            [round_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionRound".into(),
            id: round_id.to_string(),
        })?;
    map_summary(&row)
}

/// What a checkpoint still needs before the round can be completed.
fn missing_items(
    cp: &InspectionCheckpoint,
    result: Option<&InspectionResult>,
    evidence: &[InspectionEvidence],
) -> Vec<String> {
    let Some(result) = result else {
        return vec![MISSING_RESULT.to_string()];
    };
    let mut missing = Vec::new();
    if cp.requires_photo && !evidence.iter().any(|e| e.evidence_type == "photo") {
        missing.push(MISSING_PHOTO.to_string());
    }
    let has_comment = result.comment.as_deref().is_some_and(|c| !c.trim().is_empty());
    let exception_unexplained =
        cp.requires_comment_on_exception && result.result_status != "pass" && evidence.is_empty();
    if !has_comment && (exception_unexplained || reading_needs_comment(cp, &result.result_status)) {
        missing.push(MISSING_COMMENT.to_string());
    }
    missing
}

/// Number of earlier results shown with each checkpoint in the runner (history and trend).
pub const HISTORY_POINTS: i64 = 10;

/// Latest results of the checkpoint `code` of `template_id` in rounds other than `round_id`
/// (any version of the template: a checkpoint is followed by its code), newest first.
pub async fn checkpoint_history(
    db: &impl ConnectionTrait,
    template_id: i64,
    checkpoint_code: &str,
    round_id: i64,
    limit: i64,
) -> AppResult<Vec<InspectionCheckpointHistoryPoint>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.round_id, ir.scheduled_at AS round_scheduled_at, r.recorded_at, r.result_status, \
                    r.numeric_value, r.text_value, r.boolean_value, r.comment \
             FROM inspection_results r \
             JOIN inspection_checkpoints c ON c.id = r.checkpoint_id \
             JOIN inspection_rounds ir ON ir.id = r.round_id \
             WHERE ir.template_id = ? AND c.checkpoint_code = ? AND r.round_id <> ? \
             ORDER BY julianday(r.recorded_at) DESC, r.id DESC LIMIT ?",
            [
                template_id.into(),
                checkpoint_code.into(),
                round_id.into(),
                limit.clamp(1, 100).into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let b: Option<i64> = r
                .try_get("", "boolean_value")
                .map_err(|e| decode_err("boolean_value", e))?;
            Ok(InspectionCheckpointHistoryPoint {
                round_id: r.try_get("", "round_id").map_err(|e| decode_err("round_id", e))?,
                round_scheduled_at: r
                    .try_get("", "round_scheduled_at")
                    .map_err(|e| decode_err("round_scheduled_at", e))?,
                recorded_at: r.try_get("", "recorded_at").map_err(|e| decode_err("recorded_at", e))?,
                result_status: r
                    .try_get("", "result_status")
                    .map_err(|e| decode_err("result_status", e))?,
                numeric_value: r
                    .try_get("", "numeric_value")
                    .map_err(|e| decode_err("numeric_value", e))?,
                text_value: r.try_get("", "text_value").map_err(|e| decode_err("text_value", e))?,
                boolean_value: b.map(|x| x != 0),
                comment: r.try_get("", "comment").map_err(|e| decode_err("comment", e))?,
            })
        })
        .collect()
}

/// Everything the runner screen shows for one round.
pub async fn get_inspection_round_runner(db: &DatabaseConnection, round_id: i64) -> AppResult<InspectionRoundRunner> {
    let summary = get_round_summary(db, round_id).await?;
    let checkpoints = super::queries::list_inspection_checkpoints(
        db,
        InspectionCheckpointsFilter {
            template_version_id: Some(summary.round.template_version_id),
        },
    )
    .await?;
    let results = super::results::list_inspection_results(
        db,
        InspectionResultsFilter {
            round_id: Some(round_id),
        },
    )
    .await?;
    let anomalies = super::results::list_inspection_anomalies(
        db,
        InspectionAnomaliesFilter {
            round_id: Some(round_id),
        },
    )
    .await?;

    let assets = load_asset_labels(db, checkpoints.iter().filter_map(|c| c.asset_id).collect()).await?;
    let result_ids: Vec<String> = results.iter().map(|r| r.entity_sync_id.clone()).collect();
    let states = row_sync_states(db, "inspection_results", &result_ids).await?;

    let mut out = Vec::with_capacity(checkpoints.len());
    for cp in checkpoints {
        let result = results.iter().find(|r| r.checkpoint_id == cp.id).cloned();
        let evidence = match &result {
            Some(r) => {
                super::results::list_inspection_evidence(db, InspectionEvidenceFilter { result_id: Some(r.id) }).await?
            }
            None => Vec::new(),
        };
        let open_anomalies = result
            .as_ref()
            .map(|r| {
                anomalies
                    .iter()
                    .filter(|a| a.result_id == Some(r.id) && a.resolution_status == "open")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let sync_state = match &result {
            None => "not_recorded".to_string(),
            Some(r) => states
                .get(&r.entity_sync_id)
                .copied()
                .unwrap_or(ROW_SYNC_SYNCED)
                .to_string(),
        };
        let missing = missing_items(&cp, result.as_ref(), &evidence);
        let history = checkpoint_history(
            db,
            summary.round.template_id,
            &cp.checkpoint_code,
            round_id,
            HISTORY_POINTS,
        )
        .await?;
        let (asset_code, asset_name) = cp
            .asset_id
            .and_then(|id| assets.get(&id).cloned())
            .map_or((None, None), |(c, n)| (Some(c), Some(n)));
        out.push(InspectionRunnerCheckpoint {
            checkpoint: cp,
            asset_code,
            asset_name,
            result,
            evidence,
            open_anomalies,
            sync_state,
            missing,
            history,
        });
    }
    Ok(InspectionRoundRunner {
        summary,
        checkpoints: out,
    })
}

async fn load_asset_labels(db: &impl ConnectionTrait, ids: Vec<i64>) -> AppResult<HashMap<i64, (String, String)>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT id, asset_id_code, name FROM equipment WHERE id IN ({})",
                vec!["?"; ids.len()].join(", ")
            ),
            ids.into_iter().map(sea_orm::Value::from).collect::<Vec<_>>(),
        ))
        .await?;
    for r in rows {
        let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = r
            .try_get("", "asset_id_code")
            .map_err(|e| decode_err("asset_id_code", e))?;
        let name: String = r.try_get("", "name").map_err(|e| decode_err("name", e))?;
        out.insert(id, (code, name));
    }
    Ok(out)
}

async fn load_round_checked(
    db: &impl ConnectionTrait,
    input: &InspectionRoundActionInput,
) -> AppResult<InspectionRound> {
    let round = get_inspection_round_by_id(db, input.round_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionRound".into(),
            id: input.round_id.to_string(),
        })?;
    if round.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on inspection_rounds.".into(),
        ]));
    }
    Ok(round)
}

async fn write_round(
    db: &impl ConnectionTrait,
    round: &InspectionRound,
    set_clause: &str,
    mut values: Vec<sea_orm::Value>,
) -> AppResult<()> {
    values.push(round.id.into());
    values.push(round.row_version.into());
    let aff = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE inspection_rounds SET {set_clause}, row_version = row_version + 1 \
                 WHERE id = ? AND row_version = ?"
            ),
            values,
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Concurrent update on inspection_rounds.".into(),
        ]));
    }
    Ok(())
}

/// `scheduled|released → in_progress`, stamping `started_at`.
pub async fn start_inspection_round(
    db: &DatabaseConnection,
    input: InspectionRoundActionInput,
) -> AppResult<InspectionRoundSummary> {
    let txn = db.begin().await?;
    let round = load_round_checked(&txn, &input).await?;
    if round.status == "in_progress" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_ROUND_ALREADY_STARTED: The round is already in progress.".into(),
        ]));
    }
    if !ROUND_OPEN_STATUSES.contains(&round.status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ROUND_NOT_STARTABLE:status={0}: A round in status '{}' cannot be started.",
            round.status
        )]));
    }
    write_round(
        &txn,
        &round,
        "status = 'in_progress', started_at = COALESCE(started_at, ?)",
        vec![crate::clock::now().to_rfc3339().into()],
    )
    .await?;
    let out = get_round_summary(&txn, round.id).await?;
    txn.commit().await?;
    Ok(out)
}

/// `in_progress → completed | completed_with_findings`. Every checkpoint must be answered and
/// meet its evidence rules; the round has findings when a result is not a clean pass or an
/// anomaly is still open. The completion percentage counts the checks actually performed.
pub async fn complete_inspection_round(
    db: &DatabaseConnection,
    input: InspectionRoundActionInput,
) -> AppResult<InspectionRoundSummary> {
    let runner = get_inspection_round_runner(db, input.round_id).await?;
    let txn = db.begin().await?;
    let round = load_round_checked(&txn, &input).await?;
    if round.status != "in_progress" {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ROUND_NOT_IN_PROGRESS:status={0}: Only a round in progress can be completed (status '{}').",
            round.status
        )]));
    }
    let incomplete: Vec<String> = runner
        .checkpoints
        .iter()
        .filter(|c| !c.missing.is_empty())
        .map(|c| format!("{}: {}", c.checkpoint.checkpoint_code, c.missing.join(", ")))
        .collect();
    if !incomplete.is_empty() {
        return Err(AppError::ValidationFailed(
            std::iter::once(
                "GATE_INSP_ROUND_INCOMPLETE: The round still has unanswered checkpoints or missing evidence."
                    .to_string(),
            )
            .chain(incomplete)
            .collect(),
        ));
    }
    let total = runner.checkpoints.len();
    let performed = runner
        .checkpoints
        .iter()
        .filter(|c| {
            c.result
                .as_ref()
                .is_some_and(|r| !matches!(r.result_status.as_str(), "not_accessible" | "not_done"))
        })
        .count();
    let completion = if total == 0 {
        100.0
    } else {
        (performed as f64) * 100.0 / (total as f64)
    };
    let has_findings = runner.checkpoints.iter().any(|c| {
        !c.open_anomalies.is_empty()
            || c.result
                .as_ref()
                .is_some_and(|r| EXCEPTION_STATUSES.contains(&r.result_status.as_str()))
    });
    let status = if has_findings {
        "completed_with_findings"
    } else {
        "completed"
    };
    write_round(
        &txn,
        &round,
        "status = ?, completed_at = ?, completion_percentage = ?",
        vec![
            status.into(),
            crate::clock::now().to_rfc3339().into(),
            completion.into(),
        ],
    )
    .await?;
    let out = get_round_summary(&txn, round.id).await?;
    txn.commit().await?;
    Ok(out)
}

/// `completed_with_findings → reviewed` (or `completed → reviewed` when the template version
/// requires a review). Every anomaly must have a decision (no `open` anomaly left).
pub async fn review_inspection_round(
    db: &DatabaseConnection,
    input: InspectionRoundActionInput,
    reviewer_user_id: i64,
) -> AppResult<InspectionRoundSummary> {
    let txn = db.begin().await?;
    let round = load_round_checked(&txn, &input).await?;
    let requires_review: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_review FROM inspection_template_versions WHERE id = ?",
            [round.template_version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplateVersion".into(),
            id: round.template_version_id.to_string(),
        })?
        .try_get("", "requires_review")
        .map_err(|e| decode_err("requires_review", e))?;
    let reviewable = round.status == "completed_with_findings" || (round.status == "completed" && requires_review != 0);
    if !reviewable {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ROUND_NOTHING_TO_REVIEW:status={0}: A round in status '{}' has nothing to review.",
            round.status
        )]));
    }
    let open: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM inspection_anomalies WHERE round_id = ? AND resolution_status = 'open'",
            [round.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("anomaly count")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    if open > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ROUND_OPEN_ANOMALIES:count={open}: {open} anomaly(ies) still need a decision (request, work order or deferral) before the review."
        )]));
    }
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    write_round(
        &txn,
        &round,
        "status = 'reviewed', reviewed_at = ?, reviewed_by_id = ?, review_note = ?",
        vec![
            crate::clock::now().to_rfc3339().into(),
            reviewer_user_id.into(),
            note.map_or_else(|| sea_orm::Value::from(None::<String>), sea_orm::Value::from),
        ],
    )
    .await?;
    let out = get_round_summary(&txn, round.id).await?;
    txn.commit().await?;
    Ok(out)
}

/// Saves a photo for a recorded result: the file under the app data directory, the evidence row
/// and its media metadata (so the file travels to the other devices) in one transaction.
pub async fn add_inspection_photo_evidence(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    input: AddInspectionPhotoInput,
) -> AppResult<InspectionEvidence> {
    let mime = input.mime_type.trim().to_ascii_lowercase();
    if !PHOTO_MIME_TYPES.contains(&mime.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_PHOTO_TYPE:mime={mime}: Unsupported photo type '{mime}' (JPEG, PNG, WebP or HEIC)."
        )]));
    }
    if input.file_bytes.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_PHOTO_EMPTY: The photo is empty.".into(),
        ]));
    }
    if input.file_bytes.len() > MAX_PHOTO_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_PHOTO_TOO_LARGE:mb={0}: The photo is larger than {} MB.",
            MAX_PHOTO_BYTES / (1024 * 1024)
        )]));
    }
    let file_name = Path::new(&input.file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && !n.contains(".."))
        .ok_or_else(|| AppError::ValidationFailed(vec!["GATE_INSP_FILE_NAME_INVALID: Invalid file name.".into()]))?
        .to_string();

    let env = crate::sync::media::resolve_media_deployment_env();
    let tenant_id = crate::sync::media::resolve_media_tenant_id(db).await;

    let txn = db.begin().await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.round_id, ir.status FROM inspection_results r \
             JOIN inspection_rounds ir ON ir.id = r.round_id WHERE r.id = ?",
            [input.result_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionResult".into(),
            id: input.result_id.to_string(),
        })?;
    let round_id: i64 = row.try_get("", "round_id").map_err(|e| decode_err("round_id", e))?;
    let status: String = row.try_get("", "status").map_err(|e| decode_err("status", e))?;
    if !ROUND_OPEN_STATUSES.contains(&status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ROUND_EVIDENCE_CLOSED:status={status}: The round is '{status}': evidence can no longer be added."
        )]));
    }

    let evidence_sync_id = Uuid::new_v4().to_string();
    let relative_path = format!("inspection_evidence/{round_id}/{}-{file_name}", Uuid::new_v4());
    let absolute = app_data_dir.join(&relative_path);
    if let Some(parent) = absolute.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&absolute, &input.file_bytes)?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_evidence (result_id, evidence_type, file_path_or_value, captured_at, \
         entity_sync_id, row_version) VALUES (?, 'photo', ?, ?, ?, 1)",
        [
            input.result_id.into(),
            relative_path.clone().into(),
            crate::clock::now().to_rfc3339().into(),
            evidence_sync_id.clone().into(),
        ],
    ))
    .await?;
    let media = crate::sync::media::build_media_attachment(
        env,
        &tenant_id,
        "inspection_evidence",
        &evidence_sync_id,
        &file_name,
        &mime,
        &input.file_bytes,
        Some(relative_path),
        None,
    )?;
    crate::sync::media::upsert_media_attachment_row(&txn, &media).await?;
    sync_auto_anomaly(&txn, round_id, input.result_id, ANOMALY_MISSING_PHOTO, false, 3, "").await?;

    let evidence = super::results::list_inspection_evidence(
        &txn,
        InspectionEvidenceFilter {
            result_id: Some(input.result_id),
        },
    )
    .await?
    .into_iter()
    .find(|e| e.entity_sync_id == evidence_sync_id)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("inserted evidence not found")))?;
    txn.commit().await?;
    Ok(evidence)
}

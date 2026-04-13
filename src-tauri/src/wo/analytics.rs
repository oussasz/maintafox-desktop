//! WO analytics snapshot — denormalized closed-WO payload for reliability,
//! cost, and schedule analytics.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S2.
//!
//! Functions:
//!   get_wo_analytics_snapshot — assembles the full snapshot from work_orders + all sub-entities.

use crate::errors::{AppError, AppResult};
use crate::wo::closeout::{self, WoFailureDetail, WoVerification};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Denormalized analytics snapshot for a closed (or technically_verified) WO.
/// This is the contract consumed by future analytics, RAMS, and budget modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoAnalyticsSnapshot {
    pub wo_id: i64,
    pub wo_code: String,
    pub type_code: String,
    pub asset_id: Option<i64>,
    pub asset_code: Option<String>,
    pub entity_id: Option<i64>,
    pub urgency_level: Option<i64>,
    pub source_di_id: Option<i64>,
    // Execution times
    pub submitted_at: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub mechanically_completed_at: Option<String>,
    pub technically_verified_at: Option<String>,
    pub closed_at: Option<String>,
    // Time segments
    pub expected_duration_hours: Option<f64>,
    pub actual_duration_hours: Option<f64>,
    pub active_labor_hours: f64,
    pub total_waiting_hours: f64,
    pub downtime_hours: f64,
    pub schedule_deviation_hours: Option<f64>,
    // Costs
    pub labor_cost: f64,
    pub parts_cost: f64,
    pub service_cost: f64,
    pub total_cost: f64,
    // Close-out evidence
    pub recurrence_risk_level: Option<String>,
    pub root_cause_summary: Option<String>,
    pub corrective_action_summary: Option<String>,
    pub failure_details: Vec<WoFailureDetail>,
    pub verifications: Vec<WoVerification>,
    // Counts
    pub reopen_count: i64,
    pub labor_entries_count: i64,
    pub parts_entries_count: i64,
    pub attachment_count: i64,
    pub task_count: i64,
    pub mandatory_task_count: i64,
    pub completed_task_count: i64,
    pub delay_segment_count: i64,
    pub downtime_segment_count: i64,
    // Planning quality
    pub was_planned: bool,
    pub parts_actuals_confirmed: bool,
    // Module integration stubs
    pub pm_occurrence_id: Option<i64>,
    pub permit_ids: Vec<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WO analytics row decode failed for column '{column}': {e}"
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// get_wo_analytics_snapshot
// ═══════════════════════════════════════════════════════════════════════════════

/// Assemble a full analytics snapshot for a WO.
/// WO must be in `closed` or `technically_verified` state.
pub async fn get_wo_analytics_snapshot(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<WoAnalyticsSnapshot> {
    // ── Load core WO data with joins ──────────────────────────────────────
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                wo.id AS wo_id, \
                wo.code AS wo_code, \
                wot.code AS type_code, \
                wo.equipment_id AS asset_id, \
                eq.asset_id_code AS asset_code, \
                wo.entity_id, \
                ul.level AS urgency_level, \
                wo.source_di_id, \
                wo.created_at AS submitted_at, \
                wo.actual_start, \
                wo.actual_end, \
                wo.mechanically_completed_at, \
                wo.technically_verified_at, \
                wo.closed_at, \
                wo.expected_duration_hours, \
                wo.actual_duration_hours, \
                COALESCE(wo.active_labor_hours, 0.0) AS active_labor_hours, \
                COALESCE(wo.total_waiting_hours, 0.0) AS total_waiting_hours, \
                COALESCE(wo.downtime_hours, 0.0) AS downtime_hours, \
                COALESCE(wo.labor_cost, 0.0) AS labor_cost, \
                COALESCE(wo.parts_cost, 0.0) AS parts_cost, \
                COALESCE(wo.service_cost, COALESCE(wo.service_cost_input, 0.0)) AS service_cost, \
                COALESCE(wo.total_cost, 0.0) AS total_cost, \
                wo.recurrence_risk_level, \
                wo.root_cause_summary, \
                wo.corrective_action_summary, \
                COALESCE(wo.reopen_count, 0) AS reopen_count, \
                wo.parts_actuals_confirmed, \
                wo.planned_start, \
                wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_types    wot ON wot.id = wo.type_id \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             LEFT JOIN urgency_levels  ul ON ul.id  = wo.urgency_id \
             LEFT JOIN equipment       eq ON eq.id  = wo.equipment_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    // ── Guard: must be closed or technically_verified ──────────────────────
    let status_code: String = row
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;

    if !matches!(status_code.as_str(), "closed" | "technically_verified") {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le snapshot analytique n'est disponible que pour les OT clotures ou verifies \
             techniquement. Statut actuel : '{status_code}'."
        )]));
    }

    // ── Schedule deviation ────────────────────────────────────────────────
    let schedule_deviation = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ROUND((JULIANDAY(wo.actual_start) - JULIANDAY(wo.planned_start)) * 24, 2) \
                AS deviation \
             FROM work_orders wo \
             WHERE wo.id = ? AND wo.actual_start IS NOT NULL AND wo.planned_start IS NOT NULL",
            [wo_id.into()],
        ))
        .await?;

    let schedule_deviation_hours: Option<f64> = schedule_deviation
        .and_then(|r| r.try_get::<Option<f64>>("", "deviation").ok())
        .flatten();

    // ── Sub-entity counts ─────────────────────────────────────────────────
    let counts_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT \
                (SELECT COUNT(*) FROM work_order_interveners WHERE work_order_id = ?) AS labor_cnt, \
                (SELECT COUNT(*) FROM work_order_parts WHERE work_order_id = ?) AS parts_cnt, \
                (SELECT COUNT(*) FROM work_order_attachments WHERE work_order_id = ?) AS attach_cnt, \
                (SELECT COUNT(*) FROM work_order_tasks WHERE work_order_id = ?) AS task_cnt, \
                (SELECT COUNT(*) FROM work_order_tasks WHERE work_order_id = ? AND is_mandatory = 1) AS mandatory_cnt, \
                (SELECT COUNT(*) FROM work_order_tasks WHERE work_order_id = ? AND is_completed = 1) AS completed_cnt, \
                (SELECT COUNT(*) FROM work_order_delay_segments WHERE work_order_id = ?) AS delay_cnt, \
                (SELECT COUNT(*) FROM work_order_downtime_segments WHERE work_order_id = ?) AS downtime_cnt",
            [
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
                wo_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Sub-entity counts query returned no row"))
        })?;

    // ── Failure details + verifications ───────────────────────────────────
    let failure_details = closeout::get_failure_details(db, wo_id).await?;
    let verifications = closeout::get_verifications(db, wo_id).await?;

    // ── Planning quality: was_planned ─────────────────────────────────────
    let planned_start: Option<String> = row
        .try_get::<Option<String>>("", "planned_start")
        .map_err(|e| decode_err("planned_start", e))?;
    let actual_start: Option<String> = row
        .try_get::<Option<String>>("", "actual_start")
        .map_err(|e| decode_err("actual_start", e))?;
    let was_planned = planned_start.is_some() && actual_start.is_some();

    let pac: i64 = row
        .try_get::<i64>("", "parts_actuals_confirmed")
        .map_err(|e| decode_err("parts_actuals_confirmed", e))?;

    // ── Assemble snapshot ─────────────────────────────────────────────────
    Ok(WoAnalyticsSnapshot {
        wo_id: row
            .try_get::<i64>("", "wo_id")
            .map_err(|e| decode_err("wo_id", e))?,
        wo_code: row
            .try_get::<String>("", "wo_code")
            .map_err(|e| decode_err("wo_code", e))?,
        type_code: row
            .try_get::<String>("", "type_code")
            .map_err(|e| decode_err("type_code", e))?,
        asset_id: row
            .try_get::<Option<i64>>("", "asset_id")
            .map_err(|e| decode_err("asset_id", e))?,
        asset_code: row
            .try_get::<Option<String>>("", "asset_code")
            .map_err(|e| decode_err("asset_code", e))?,
        entity_id: row
            .try_get::<Option<i64>>("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?,
        urgency_level: row
            .try_get::<Option<i64>>("", "urgency_level")
            .map_err(|e| decode_err("urgency_level", e))?,
        source_di_id: row
            .try_get::<Option<i64>>("", "source_di_id")
            .map_err(|e| decode_err("source_di_id", e))?,
        submitted_at: row
            .try_get::<Option<String>>("", "submitted_at")
            .map_err(|e| decode_err("submitted_at", e))?,
        actual_start,
        actual_end: row
            .try_get::<Option<String>>("", "actual_end")
            .map_err(|e| decode_err("actual_end", e))?,
        mechanically_completed_at: row
            .try_get::<Option<String>>("", "mechanically_completed_at")
            .map_err(|e| decode_err("mechanically_completed_at", e))?,
        technically_verified_at: row
            .try_get::<Option<String>>("", "technically_verified_at")
            .map_err(|e| decode_err("technically_verified_at", e))?,
        closed_at: row
            .try_get::<Option<String>>("", "closed_at")
            .map_err(|e| decode_err("closed_at", e))?,
        expected_duration_hours: row
            .try_get::<Option<f64>>("", "expected_duration_hours")
            .map_err(|e| decode_err("expected_duration_hours", e))?,
        actual_duration_hours: row
            .try_get::<Option<f64>>("", "actual_duration_hours")
            .map_err(|e| decode_err("actual_duration_hours", e))?,
        active_labor_hours: row
            .try_get::<f64>("", "active_labor_hours")
            .map_err(|e| decode_err("active_labor_hours", e))?,
        total_waiting_hours: row
            .try_get::<f64>("", "total_waiting_hours")
            .map_err(|e| decode_err("total_waiting_hours", e))?,
        downtime_hours: row
            .try_get::<f64>("", "downtime_hours")
            .map_err(|e| decode_err("downtime_hours", e))?,
        schedule_deviation_hours,
        labor_cost: row
            .try_get::<f64>("", "labor_cost")
            .map_err(|e| decode_err("labor_cost", e))?,
        parts_cost: row
            .try_get::<f64>("", "parts_cost")
            .map_err(|e| decode_err("parts_cost", e))?,
        service_cost: row
            .try_get::<f64>("", "service_cost")
            .map_err(|e| decode_err("service_cost", e))?,
        total_cost: row
            .try_get::<f64>("", "total_cost")
            .map_err(|e| decode_err("total_cost", e))?,
        recurrence_risk_level: row
            .try_get::<Option<String>>("", "recurrence_risk_level")
            .map_err(|e| decode_err("recurrence_risk_level", e))?,
        root_cause_summary: row
            .try_get::<Option<String>>("", "root_cause_summary")
            .map_err(|e| decode_err("root_cause_summary", e))?,
        corrective_action_summary: row
            .try_get::<Option<String>>("", "corrective_action_summary")
            .map_err(|e| decode_err("corrective_action_summary", e))?,
        failure_details,
        verifications,
        reopen_count: row
            .try_get::<i64>("", "reopen_count")
            .map_err(|e| decode_err("reopen_count", e))?,
        labor_entries_count: counts_row
            .try_get::<i64>("", "labor_cnt")
            .map_err(|e| decode_err("labor_cnt", e))?,
        parts_entries_count: counts_row
            .try_get::<i64>("", "parts_cnt")
            .map_err(|e| decode_err("parts_cnt", e))?,
        attachment_count: counts_row
            .try_get::<i64>("", "attach_cnt")
            .map_err(|e| decode_err("attach_cnt", e))?,
        task_count: counts_row
            .try_get::<i64>("", "task_cnt")
            .map_err(|e| decode_err("task_cnt", e))?,
        mandatory_task_count: counts_row
            .try_get::<i64>("", "mandatory_cnt")
            .map_err(|e| decode_err("mandatory_cnt", e))?,
        completed_task_count: counts_row
            .try_get::<i64>("", "completed_cnt")
            .map_err(|e| decode_err("completed_cnt", e))?,
        delay_segment_count: counts_row
            .try_get::<i64>("", "delay_cnt")
            .map_err(|e| decode_err("delay_cnt", e))?,
        downtime_segment_count: counts_row
            .try_get::<i64>("", "downtime_cnt")
            .map_err(|e| decode_err("downtime_cnt", e))?,
        was_planned,
        parts_actuals_confirmed: pac != 0,
        // Module integration stubs — populated by future SPs
        pm_occurrence_id: None,
        permit_ids: vec![],
    })
}

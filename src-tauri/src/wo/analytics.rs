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
    /// Level of the shared DI / OT urgency scale.
    pub urgency_code: Option<String>,
    /// EN 13306 maintenance nature of the OT type.
    pub nature_code: Option<String>,
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
    // Links to other modules
    /// PM occurrence that generated this WO (`pm_occurrences.linked_work_order_id`).
    pub pm_occurrence_id: Option<i64>,
    pub pm_plan_code: Option<String>,
    pub pm_plan_title: Option<String>,
    pub pm_due_at: Option<String>,
    /// Work permits linked to this WO, newest first.
    pub permit_ids: Vec<i64>,
    pub permits: Vec<WoLinkedPermit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoLinkedPermit {
    pub id: i64,
    pub code: String,
    pub status: String,
    pub permit_type_code: Option<String>,
}

struct PmLink {
    occurrence_id: i64,
    plan_code: String,
    plan_title: String,
    due_at: Option<String>,
}

async fn load_pm_link(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<PmLink>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT o.id, o.due_at, p.code, p.title FROM pm_occurrences o \
             JOIN pm_plans p ON p.id = o.pm_plan_id \
             WHERE o.linked_work_order_id = ? ORDER BY o.id DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some(PmLink {
        occurrence_id: r
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("pm_occurrence.id", e))?,
        plan_code: r
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("pm_plans.code", e))?,
        plan_title: r
            .try_get::<String>("", "title")
            .map_err(|e| decode_err("pm_plans.title", e))?,
        due_at: r
            .try_get::<Option<String>>("", "due_at")
            .map_err(|e| decode_err("pm_occurrence.due_at", e))?,
    }))
}

pub(crate) async fn load_permit_links(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<WoLinkedPermit>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wp.id, wp.code, wp.status, pt.code AS permit_type_code FROM work_permits wp \
             LEFT JOIN permit_types pt ON pt.id = wp.permit_type_id \
             WHERE wp.linked_work_order_id = ? ORDER BY wp.id DESC",
            [wo_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(WoLinkedPermit {
                id: r
                    .try_get::<i64>("", "id")
                    .map_err(|e| decode_err("work_permits.id", e))?,
                code: r
                    .try_get::<String>("", "code")
                    .map_err(|e| decode_err("work_permits.code", e))?,
                status: r
                    .try_get::<String>("", "status")
                    .map_err(|e| decode_err("work_permits.status", e))?,
                permit_type_code: r
                    .try_get::<Option<String>>("", "permit_type_code")
                    .map_err(|e| decode_err("permit_types.code", e))?,
            })
        })
        .collect()
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
pub async fn get_wo_analytics_snapshot(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoAnalyticsSnapshot> {
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
                ul.code AS urgency_code, wot.semantic_tag AS nature_code, \
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
             JOIN reference_values    wot ON wot.id = wo.type_id \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             LEFT JOIN reference_values ul ON ul.id = wo.urgency_id \
             LEFT JOIN equipment       eq ON eq.id  = wo.equipment_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    // ── Guard: must be closed or technically verified (Option B keeps status=completed) ──
    let status_code: String = row
        .try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    let technically_verified_at: Option<String> = row
        .try_get::<Option<String>>("", "technically_verified_at")
        .map_err(|e| decode_err("technically_verified_at", e))?;
    let is_technically_verified = matches!(status_code.as_str(), "technically_verified")
        || technically_verified_at
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    if status_code != "closed" && !is_technically_verified {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_WO_ANALYTICS_STATUS:status={status_code}: Le snapshot analytique n'est disponible que pour les OT clotures ou verifies \
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
                (SELECT COUNT(*) FROM work_order_parts WHERE work_order_id = ? AND consumption_status <> 'cancelled') AS parts_cnt, \
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Sub-entity counts query returned no row")))?;

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
    let pm = load_pm_link(db, wo_id).await?;
    let permits = load_permit_links(db, wo_id).await?;

    Ok(WoAnalyticsSnapshot {
        wo_id: row.try_get::<i64>("", "wo_id").map_err(|e| decode_err("wo_id", e))?,
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
        urgency_code: row
            .try_get::<Option<String>>("", "urgency_code")
            .map_err(|e| decode_err("urgency_code", e))?,
        // The nature the stored tag names (`MaintenanceNature::parse`), never the raw tag.
        nature_code: row
            .try_get::<Option<String>>("", "nature_code")
            .map_err(|e| decode_err("nature_code", e))?
            .as_deref()
            .and_then(crate::wo::types::MaintenanceNature::parse)
            .map(|n| n.as_str().to_string()),
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
        technically_verified_at,
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
        pm_occurrence_id: pm.as_ref().map(|p| p.occurrence_id),
        pm_plan_code: pm.as_ref().map(|p| p.plan_code.clone()),
        pm_plan_title: pm.as_ref().map(|p| p.plan_title.clone()),
        pm_due_at: pm.and_then(|p| p.due_at),
        permit_ids: permits.iter().map(|p| p.id).collect(),
        permits,
    })
}

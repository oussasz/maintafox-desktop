//! WO domain types, state machine, and code generation.
//!
//! Phase 2 - Sub-phase 05 - File 01 - Sprint S1.
//!
//! Implements the full 12-state PRD §6.5 workflow:
//!   Draft → Awaiting Approval → Planned → Ready To Schedule → Assigned →
//!   Waiting For Prerequisite → In Progress → Paused → Mechanically Complete →
//!   Technically Verified → Closed; any pre-close state → Cancelled
//!
//! The state machine is enforced in Rust — the frontend never decides validity
//! of a transition. `guard_wo_transition` is the single authority.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// WoStatus — PRD §6.5 exact 12-state enum
// ═══════════════════════════════════════════════════════════════════════════════

/// All legal states for a work order, per PRD §6.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WoStatus {
    Draft,
    AwaitingApproval,
    Planned,
    ReadyToSchedule,
    Assigned,
    WaitingForPrerequisite,
    InProgress,
    Paused,
    MechanicallyComplete,
    TechnicallyVerified,
    Closed,
    Cancelled,
}

impl WoStatus {
    /// Database-persisted snake_case representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Planned => "planned",
            Self::ReadyToSchedule => "ready_to_schedule",
            Self::Assigned => "assigned",
            Self::WaitingForPrerequisite => "waiting_for_prerequisite",
            Self::InProgress => "in_progress",
            Self::Paused => "paused",
            Self::MechanicallyComplete => "mechanically_complete",
            Self::TechnicallyVerified => "technically_verified",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse from the DB-stored snake_case string.
    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(Self::Draft),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "planned" => Ok(Self::Planned),
            "ready_to_schedule" => Ok(Self::ReadyToSchedule),
            "assigned" => Ok(Self::Assigned),
            "waiting_for_prerequisite" => Ok(Self::WaitingForPrerequisite),
            "in_progress" => Ok(Self::InProgress),
            "paused" => Ok(Self::Paused),
            "mechanically_complete" => Ok(Self::MechanicallyComplete),
            "technically_verified" => Ok(Self::TechnicallyVerified),
            "closed" => Ok(Self::Closed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("Unknown WO status: '{other}'")),
        }
    }

    /// Exact PRD §6.5 transition table. No additions, no omissions.
    pub fn allowed_transitions(&self) -> &'static [WoStatus] {
        match self {
            Self::Draft => &[Self::AwaitingApproval, Self::Planned, Self::Cancelled],
            Self::AwaitingApproval => &[Self::Planned, Self::Cancelled],
            Self::Planned => &[Self::ReadyToSchedule, Self::Cancelled],
            Self::ReadyToSchedule => &[Self::Assigned, Self::Cancelled],
            Self::Assigned => &[
                Self::WaitingForPrerequisite,
                Self::InProgress,
                Self::Cancelled,
            ],
            Self::WaitingForPrerequisite => &[
                Self::Assigned,
                Self::InProgress,
                Self::Cancelled,
            ],
            Self::InProgress => &[
                Self::Paused,
                Self::MechanicallyComplete,
                Self::Cancelled,
            ],
            Self::Paused => &[Self::InProgress, Self::Cancelled],
            Self::MechanicallyComplete => &[
                Self::TechnicallyVerified,
                Self::InProgress,
                Self::Cancelled,
            ],
            Self::TechnicallyVerified => &[Self::Closed],
            Self::Closed => &[],
            Self::Cancelled => &[],
        }
    }

    /// Terminal states: no further transitions allowed.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Closed | Self::Cancelled)
    }

    /// Active execution states (on the shop floor).
    pub fn is_executing(&self) -> bool {
        matches!(self, Self::InProgress | Self::Paused)
    }

    /// Whether transitioning FROM this state to Closed requires step-up
    /// reauthentication (closure quality gate).
    pub fn requires_step_up_for_close(&self) -> bool {
        matches!(self, Self::TechnicallyVerified)
    }
}

impl std::fmt::Display for WoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// WoMacroState — PRD §6.5 macro grouping
// ═══════════════════════════════════════════════════════════════════════════════

/// High-level grouping for WO statuses used in dashboards and filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WoMacroState {
    Open,
    Executing,
    Completed,
    Closed,
    Cancelled,
}

impl WoMacroState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "open" => Ok(Self::Open),
            "executing" => Ok(Self::Executing),
            "completed" => Ok(Self::Completed),
            "closed" => Ok(Self::Closed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("Unknown WO macro state: '{other}'")),
        }
    }
}

impl std::fmt::Display for WoMacroState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// State machine guard
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that transitioning from `from` to `to` is allowed by the PRD §6.5
/// transition table. Returns `Err` with a descriptive message on illegal moves.
pub fn guard_wo_transition(from: &WoStatus, to: &WoStatus) -> Result<(), String> {
    if from.allowed_transitions().contains(to) {
        Ok(())
    } else {
        Err(format!(
            "Illegal WO state transition: '{}' -> '{}'",
            from.as_str(),
            to.as_str()
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// WorkOrder — full row struct matching DDL (migration 022)
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete work order record for reads.
/// Matches all columns in `work_orders` (migration 022).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrder {
    pub id: i64,
    pub code: String,
    // Classification
    pub type_id: i64,
    pub status_id: i64,
    // Asset context
    pub equipment_id: Option<i64>,
    pub component_id: Option<i64>,
    pub location_id: Option<i64>,
    // People
    pub requester_id: Option<i64>,
    pub source_di_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub planner_id: Option<i64>,
    pub approver_id: Option<i64>,
    pub assigned_group_id: Option<i64>,
    pub primary_responsible_id: Option<i64>,
    // Urgency
    pub urgency_id: Option<i64>,
    // Core description
    pub title: String,
    pub description: Option<String>,
    // Timing
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub scheduled_at: Option<String>,
    pub actual_start: Option<String>,
    pub actual_end: Option<String>,
    pub mechanically_completed_at: Option<String>,
    pub technically_verified_at: Option<String>,
    pub closed_at: Option<String>,
    pub cancelled_at: Option<String>,
    // Duration accumulators
    pub expected_duration_hours: Option<f64>,
    pub actual_duration_hours: Option<f64>,
    pub active_labor_hours: Option<f64>,
    pub total_waiting_hours: Option<f64>,
    pub downtime_hours: Option<f64>,
    // Cost accumulators
    pub labor_cost: Option<f64>,
    pub parts_cost: Option<f64>,
    pub service_cost: Option<f64>,
    pub total_cost: Option<f64>,
    // Close-out evidence
    pub recurrence_risk_level: Option<String>,
    pub production_impact_id: Option<i64>,
    pub root_cause_summary: Option<String>,
    pub corrective_action_summary: Option<String>,
    pub verification_method: Option<String>,
    // Metadata
    pub notes: Option<String>,
    pub cancel_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    // Join fields (populated by queries, not stored in DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsible_username: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// WoTransitionRow — state transition log entry
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoTransitionRow {
    pub id: i64,
    pub wo_id: i64,
    pub from_status: String,
    pub to_status: String,
    pub action: String,
    pub actor_id: Option<i64>,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
    pub acted_at: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command input structs
// ═══════════════════════════════════════════════════════════════════════════════

/// Input for creating a new work order (always starts as draft).
#[derive(Debug, Clone, Deserialize)]
pub struct WoCreateInput {
    pub type_id: i64,
    pub equipment_id: Option<i64>,
    pub location_id: Option<i64>,
    pub source_di_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub planner_id: Option<i64>,
    pub urgency_id: Option<i64>,
    pub title: String,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub creator_id: i64,
}

/// Input for updating a WO that is still in draft status.
#[derive(Debug, Clone, Deserialize)]
pub struct WoDraftUpdateInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub title: Option<String>,
    pub type_id: Option<i64>,
    pub equipment_id: Option<i64>,
    pub location_id: Option<i64>,
    pub description: Option<String>,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub expected_duration_hours: Option<f64>,
    pub notes: Option<String>,
    pub urgency_id: Option<i64>,
}

/// Input for cancelling a work order.
#[derive(Debug, Clone, Deserialize)]
pub struct WoCancelInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub actor_id: i64,
    pub cancel_reason: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "work_orders row decode failed for column '{column}': {e}"
    ))
}

/// Map a sea-orm `QueryResult` row to a `WorkOrder`.
/// Join columns (status_code, type_label, etc.) are optional — they are only
/// present when the query includes the relevant JOINs.
pub fn map_work_order(row: &QueryResult) -> AppResult<WorkOrder> {
    Ok(WorkOrder {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        type_id: row
            .try_get::<i64>("", "type_id")
            .map_err(|e| decode_err("type_id", e))?,
        status_id: row
            .try_get::<i64>("", "status_id")
            .map_err(|e| decode_err("status_id", e))?,
        equipment_id: row
            .try_get::<Option<i64>>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        component_id: row
            .try_get::<Option<i64>>("", "component_id")
            .map_err(|e| decode_err("component_id", e))?,
        location_id: row
            .try_get::<Option<i64>>("", "location_id")
            .map_err(|e| decode_err("location_id", e))?,
        requester_id: row
            .try_get::<Option<i64>>("", "requester_id")
            .map_err(|e| decode_err("requester_id", e))?,
        source_di_id: row
            .try_get::<Option<i64>>("", "source_di_id")
            .map_err(|e| decode_err("source_di_id", e))?,
        entity_id: row
            .try_get::<Option<i64>>("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?,
        planner_id: row
            .try_get::<Option<i64>>("", "planner_id")
            .map_err(|e| decode_err("planner_id", e))?,
        approver_id: row
            .try_get::<Option<i64>>("", "approver_id")
            .map_err(|e| decode_err("approver_id", e))?,
        assigned_group_id: row
            .try_get::<Option<i64>>("", "assigned_group_id")
            .map_err(|e| decode_err("assigned_group_id", e))?,
        primary_responsible_id: row
            .try_get::<Option<i64>>("", "primary_responsible_id")
            .map_err(|e| decode_err("primary_responsible_id", e))?,
        urgency_id: row
            .try_get::<Option<i64>>("", "urgency_id")
            .map_err(|e| decode_err("urgency_id", e))?,
        title: row
            .try_get::<String>("", "title")
            .map_err(|e| decode_err("title", e))?,
        description: row
            .try_get::<Option<String>>("", "description")
            .map_err(|e| decode_err("description", e))?,
        planned_start: row
            .try_get::<Option<String>>("", "planned_start")
            .map_err(|e| decode_err("planned_start", e))?,
        planned_end: row
            .try_get::<Option<String>>("", "planned_end")
            .map_err(|e| decode_err("planned_end", e))?,
        scheduled_at: row
            .try_get::<Option<String>>("", "scheduled_at")
            .map_err(|e| decode_err("scheduled_at", e))?,
        actual_start: row
            .try_get::<Option<String>>("", "actual_start")
            .map_err(|e| decode_err("actual_start", e))?,
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
        cancelled_at: row
            .try_get::<Option<String>>("", "cancelled_at")
            .map_err(|e| decode_err("cancelled_at", e))?,
        expected_duration_hours: row
            .try_get::<Option<f64>>("", "expected_duration_hours")
            .map_err(|e| decode_err("expected_duration_hours", e))?,
        actual_duration_hours: row
            .try_get::<Option<f64>>("", "actual_duration_hours")
            .map_err(|e| decode_err("actual_duration_hours", e))?,
        active_labor_hours: row
            .try_get::<Option<f64>>("", "active_labor_hours")
            .map_err(|e| decode_err("active_labor_hours", e))?,
        total_waiting_hours: row
            .try_get::<Option<f64>>("", "total_waiting_hours")
            .map_err(|e| decode_err("total_waiting_hours", e))?,
        downtime_hours: row
            .try_get::<Option<f64>>("", "downtime_hours")
            .map_err(|e| decode_err("downtime_hours", e))?,
        labor_cost: row
            .try_get::<Option<f64>>("", "labor_cost")
            .map_err(|e| decode_err("labor_cost", e))?,
        parts_cost: row
            .try_get::<Option<f64>>("", "parts_cost")
            .map_err(|e| decode_err("parts_cost", e))?,
        service_cost: row
            .try_get::<Option<f64>>("", "service_cost")
            .map_err(|e| decode_err("service_cost", e))?,
        total_cost: row
            .try_get::<Option<f64>>("", "total_cost")
            .map_err(|e| decode_err("total_cost", e))?,
        recurrence_risk_level: row
            .try_get::<Option<String>>("", "recurrence_risk_level")
            .map_err(|e| decode_err("recurrence_risk_level", e))?,
        production_impact_id: row
            .try_get::<Option<i64>>("", "production_impact_id")
            .map_err(|e| decode_err("production_impact_id", e))?,
        root_cause_summary: row
            .try_get::<Option<String>>("", "root_cause_summary")
            .map_err(|e| decode_err("root_cause_summary", e))?,
        corrective_action_summary: row
            .try_get::<Option<String>>("", "corrective_action_summary")
            .map_err(|e| decode_err("corrective_action_summary", e))?,
        verification_method: row
            .try_get::<Option<String>>("", "verification_method")
            .map_err(|e| decode_err("verification_method", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        cancel_reason: row
            .try_get::<Option<String>>("", "cancel_reason")
            .map_err(|e| decode_err("cancel_reason", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
        // Join fields — optional, depend on query shape
        status_code: row.try_get::<Option<String>>("", "status_code").unwrap_or(None),
        status_label: row.try_get::<Option<String>>("", "status_label").unwrap_or(None),
        status_color: row.try_get::<Option<String>>("", "status_color").unwrap_or(None),
        type_code: row.try_get::<Option<String>>("", "type_code").unwrap_or(None),
        type_label: row.try_get::<Option<String>>("", "type_label").unwrap_or(None),
        urgency_level: row.try_get::<Option<i64>>("", "urgency_level").unwrap_or(None),
        urgency_label: row.try_get::<Option<String>>("", "urgency_label").unwrap_or(None),
        urgency_color: row.try_get::<Option<String>>("", "urgency_color").unwrap_or(None),
        asset_code: row.try_get::<Option<String>>("", "asset_code").unwrap_or(None),
        asset_label: row.try_get::<Option<String>>("", "asset_label").unwrap_or(None),
        planner_username: row.try_get::<Option<String>>("", "planner_username").unwrap_or(None),
        responsible_username: row.try_get::<Option<String>>("", "responsible_username").unwrap_or(None),
    })
}

/// Map a sea-orm `QueryResult` row to a `WoTransitionRow`.
pub fn map_wo_transition_row(row: &QueryResult) -> AppResult<WoTransitionRow> {
    Ok(WoTransitionRow {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        wo_id: row
            .try_get::<i64>("", "wo_id")
            .map_err(|e| decode_err("wo_id", e))?,
        from_status: row
            .try_get::<String>("", "from_status")
            .map_err(|e| decode_err("from_status", e))?,
        to_status: row
            .try_get::<String>("", "to_status")
            .map_err(|e| decode_err("to_status", e))?,
        action: row
            .try_get::<String>("", "action")
            .map_err(|e| decode_err("action", e))?,
        actor_id: row
            .try_get::<Option<i64>>("", "actor_id")
            .map_err(|e| decode_err("actor_id", e))?,
        reason_code: row
            .try_get::<Option<String>>("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        acted_at: row
            .try_get::<String>("", "acted_at")
            .map_err(|e| decode_err("acted_at", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// WO code generator
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate the next unique WO code in the format `WOR-NNNN`.
/// Reads the current max sequence from the database and increments.
/// The code is never recycled after deletion or cancellation.
pub async fn generate_wo_code(db: &DatabaseConnection) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(CAST(SUBSTR(code, 5) AS INTEGER)), 0) + 1 AS next_seq \
             FROM work_orders WHERE code LIKE 'WOR-%'"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "WO code sequence query returned no rows"
            ))
        })?;

    let next_seq: i64 = row
        .try_get::<i64>("", "next_seq")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("WO code decode error: {e}")))?;

    Ok(format!("WOR-{next_seq:04}"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Status string round-trip ──────────────────────────────────────────

    #[test]
    fn test_all_status_round_trip() {
        let all = [
            WoStatus::Draft,
            WoStatus::AwaitingApproval,
            WoStatus::Planned,
            WoStatus::ReadyToSchedule,
            WoStatus::Assigned,
            WoStatus::WaitingForPrerequisite,
            WoStatus::InProgress,
            WoStatus::Paused,
            WoStatus::MechanicallyComplete,
            WoStatus::TechnicallyVerified,
            WoStatus::Closed,
            WoStatus::Cancelled,
        ];
        assert_eq!(all.len(), 12, "PRD §6.5 requires exactly 12 states");

        for status in &all {
            let s = status.as_str();
            let parsed = WoStatus::try_from_str(s).unwrap();
            assert_eq!(*status, parsed, "Round-trip failed for '{s}'");
        }
    }

    #[test]
    fn test_invalid_status_rejected() {
        assert!(WoStatus::try_from_str("invalid").is_err());
        assert!(WoStatus::try_from_str("").is_err());
        assert!(WoStatus::try_from_str("DRAFT").is_err()); // case-sensitive
    }

    // ── Transition table coverage ─────────────────────────────────────────

    #[test]
    fn test_all_valid_forward_transitions() {
        let cases = [
            (WoStatus::Draft, WoStatus::AwaitingApproval),
            (WoStatus::Draft, WoStatus::Planned),
            (WoStatus::Draft, WoStatus::Cancelled),
            (WoStatus::AwaitingApproval, WoStatus::Planned),
            (WoStatus::AwaitingApproval, WoStatus::Cancelled),
            (WoStatus::Planned, WoStatus::ReadyToSchedule),
            (WoStatus::Planned, WoStatus::Cancelled),
            (WoStatus::ReadyToSchedule, WoStatus::Assigned),
            (WoStatus::ReadyToSchedule, WoStatus::Cancelled),
            (WoStatus::Assigned, WoStatus::WaitingForPrerequisite),
            (WoStatus::Assigned, WoStatus::InProgress),
            (WoStatus::Assigned, WoStatus::Cancelled),
            (WoStatus::WaitingForPrerequisite, WoStatus::Assigned),
            (WoStatus::WaitingForPrerequisite, WoStatus::InProgress),
            (WoStatus::WaitingForPrerequisite, WoStatus::Cancelled),
            (WoStatus::InProgress, WoStatus::Paused),
            (WoStatus::InProgress, WoStatus::MechanicallyComplete),
            (WoStatus::InProgress, WoStatus::Cancelled),
            (WoStatus::Paused, WoStatus::InProgress),
            (WoStatus::Paused, WoStatus::Cancelled),
            (WoStatus::MechanicallyComplete, WoStatus::TechnicallyVerified),
            (WoStatus::MechanicallyComplete, WoStatus::InProgress),
            (WoStatus::MechanicallyComplete, WoStatus::Cancelled),
            (WoStatus::TechnicallyVerified, WoStatus::Closed),
        ];

        for (from, to) in &cases {
            assert!(
                guard_wo_transition(from, to).is_ok(),
                "Expected valid transition: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[test]
    fn test_invalid_transitions_rejected() {
        let invalid_cases = [
            (WoStatus::Draft, WoStatus::InProgress),
            (WoStatus::Draft, WoStatus::Closed),
            (WoStatus::Closed, WoStatus::Draft),
            (WoStatus::Closed, WoStatus::InProgress),
            (WoStatus::Cancelled, WoStatus::Draft),
            (WoStatus::Cancelled, WoStatus::InProgress),
            (WoStatus::TechnicallyVerified, WoStatus::Cancelled),
            (WoStatus::InProgress, WoStatus::Draft),
            (WoStatus::Planned, WoStatus::InProgress),
        ];

        for (from, to) in &invalid_cases {
            assert!(
                guard_wo_transition(from, to).is_err(),
                "Expected invalid transition: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[test]
    fn test_terminal_states_have_no_outbound_transitions() {
        assert!(WoStatus::Closed.allowed_transitions().is_empty());
        assert!(WoStatus::Cancelled.allowed_transitions().is_empty());
    }

    // ── Cancelled reachability ────────────────────────────────────────────

    #[test]
    fn test_cancelled_reachable_from_all_pre_terminal_states() {
        let pre_terminal = [
            WoStatus::Draft,
            WoStatus::AwaitingApproval,
            WoStatus::Planned,
            WoStatus::ReadyToSchedule,
            WoStatus::Assigned,
            WoStatus::WaitingForPrerequisite,
            WoStatus::InProgress,
            WoStatus::Paused,
            WoStatus::MechanicallyComplete,
        ];

        for status in &pre_terminal {
            assert!(
                status.allowed_transitions().contains(&WoStatus::Cancelled),
                "Cancelled should be reachable from {}",
                status.as_str()
            );
        }

        // TechnicallyVerified → Closed only (no cancel)
        assert!(
            !WoStatus::TechnicallyVerified
                .allowed_transitions()
                .contains(&WoStatus::Cancelled),
            "TechnicallyVerified should NOT allow Cancelled"
        );
    }

    // ── Terminal / executing / step-up flags ──────────────────────────────

    #[test]
    fn test_is_terminal() {
        assert!(WoStatus::Closed.is_terminal());
        assert!(WoStatus::Cancelled.is_terminal());
        assert!(!WoStatus::Draft.is_terminal());
        assert!(!WoStatus::InProgress.is_terminal());
    }

    #[test]
    fn test_is_executing() {
        assert!(WoStatus::InProgress.is_executing());
        assert!(WoStatus::Paused.is_executing());
        assert!(!WoStatus::Draft.is_executing());
        assert!(!WoStatus::Assigned.is_executing());
        assert!(!WoStatus::MechanicallyComplete.is_executing());
    }

    #[test]
    fn test_requires_step_up_for_close() {
        assert!(WoStatus::TechnicallyVerified.requires_step_up_for_close());
        assert!(!WoStatus::Draft.requires_step_up_for_close());
        assert!(!WoStatus::InProgress.requires_step_up_for_close());
        assert!(!WoStatus::MechanicallyComplete.requires_step_up_for_close());
    }

    // ── Macro state round-trip ────────────────────────────────────────────

    #[test]
    fn test_macro_state_round_trip() {
        let all = [
            WoMacroState::Open,
            WoMacroState::Executing,
            WoMacroState::Completed,
            WoMacroState::Closed,
            WoMacroState::Cancelled,
        ];
        assert_eq!(all.len(), 5);

        for ms in &all {
            let s = ms.as_str();
            let parsed = WoMacroState::try_from_str(s).unwrap();
            assert_eq!(*ms, parsed, "Round-trip failed for '{s}'");
        }
    }

    #[test]
    fn test_invalid_macro_state_rejected() {
        assert!(WoMacroState::try_from_str("invalid").is_err());
        assert!(WoMacroState::try_from_str("").is_err());
    }
}

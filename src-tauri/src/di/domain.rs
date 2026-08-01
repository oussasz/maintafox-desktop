//! DI domain types, state machine, and code generation.
//!
//! Lifecycle redesign — 7-state model (migration 139):
//!   Submitted → InReview → AwaitingApproval → Approved → Closed (terminal)
//!   Plus: ReturnedForClarification, Deferred as side paths.
//!
//! The state machine is enforced in Rust — the frontend never decides validity
//! of a transition. `guard_transition` is the single authority.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// DiStatus — 7-state lifecycle (migration 139)
// ═══════════════════════════════════════════════════════════════════════════════

/// All legal states for an intervention request (7-state redesign).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiStatus {
    Submitted,
    InReview,
    ReturnedForClarification,
    AwaitingApproval,
    Approved,
    Deferred,
    Closed,
}

impl DiStatus {
    /// Database-persisted snake_case representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::InReview => "in_review",
            Self::ReturnedForClarification => "returned_for_clarification",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::Deferred => "deferred",
            Self::Closed => "closed",
        }
    }

    /// Parse from the DB-stored snake_case string.
    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "submitted" => Ok(Self::Submitted),
            "in_review" => Ok(Self::InReview),
            "returned_for_clarification" => Ok(Self::ReturnedForClarification),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "deferred" => Ok(Self::Deferred),
            "closed" => Ok(Self::Closed),
            other => Err(format!("Unknown DI status: '{other}'")),
        }
    }

    /// Transition table for the 7-state lifecycle.
    pub fn allowed_transitions(&self) -> &'static [DiStatus] {
        match self {
            Self::Submitted => &[Self::InReview, Self::Closed],
            Self::InReview => &[
                Self::ReturnedForClarification,
                Self::AwaitingApproval,
                Self::Closed,
                Self::Deferred,
            ],
            Self::ReturnedForClarification => &[Self::InReview, Self::Closed],
            Self::AwaitingApproval => &[Self::Approved, Self::Closed, Self::Deferred],
            Self::Approved => &[Self::Closed, Self::Deferred],
            // Deferred restores to the saved deferred_from_status — all three are allowed.
            Self::Deferred => &[Self::InReview, Self::AwaitingApproval, Self::Approved],
            Self::Closed => &[],
        }
    }

    /// `Closed` is the sole immutable (terminal) state.
    pub fn is_immutable(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// States whose entry requires step-up reauthentication.
    /// `Approved` (approve action) and `Closed` when converting (handled in convert command).
    pub fn requires_step_up_to_enter(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

impl std::fmt::Display for DiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiOriginType — baseline seed codes only (NOT create-time authority)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Category B open catalog: `DI.ORIGIN` in `reference_values` is the SSOT for
// create/update validation. Tenant-extended codes are valid once published and
// active. This enum documents the seeded baseline set for tests/seeds; do not
// use `try_from_str` to gate intake writes.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiOriginType {
    Operator,
    Technician,
    Inspection,
    Pm,
    Iot,
    Quality,
    Hse,
    Production,
    External,
}

impl DiOriginType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Technician => "technician",
            Self::Inspection => "inspection",
            Self::Pm => "pm",
            Self::Iot => "iot",
            Self::Quality => "quality",
            Self::Hse => "hse",
            Self::Production => "production",
            Self::External => "external",
        }
    }

    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "operator" => Ok(Self::Operator),
            "technician" => Ok(Self::Technician),
            "inspection" => Ok(Self::Inspection),
            "pm" => Ok(Self::Pm),
            "iot" => Ok(Self::Iot),
            "quality" => Ok(Self::Quality),
            "hse" => Ok(Self::Hse),
            "production" => Ok(Self::Production),
            "external" => Ok(Self::External),
            other => Err(format!("Unknown DI origin type: '{other}'")),
        }
    }
}

impl std::fmt::Display for DiOriginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiUrgency
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiUrgency {
    Low,
    Medium,
    High,
    Critical,
}

impl DiUrgency {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(format!("Unknown DI urgency: '{other}'")),
        }
    }
}

impl std::fmt::Display for DiUrgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiImpactLevel
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiImpactLevel {
    Unknown,
    None,
    Minor,
    Major,
    Critical,
}

impl DiImpactLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
        }
    }

    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "unknown" => Ok(Self::Unknown),
            "none" => Ok(Self::None),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            "critical" => Ok(Self::Critical),
            other => Err(format!("Unknown DI impact level: '{other}'")),
        }
    }
}

impl std::fmt::Display for DiImpactLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// InterventionRequest — full row struct matching DDL
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete intervention request record for reads.
/// Matches all columns in `intervention_requests` (migration 017).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionRequest {
    pub id: i64,
    pub code: String,
    // Origin context
    pub asset_id: i64,
    pub sub_asset_ref: Option<String>,
    pub org_node_id: i64,
    // State
    pub status: String,
    // Triage evidence
    pub title: String,
    pub description: String,
    pub origin_type: String,
    /// Category A `DI.REQUEST_TYPE` code (default `repair`).
    pub request_type: String,
    pub symptom_code_id: Option<i64>,
    // Impact flags
    pub impact_level: String,
    pub production_impact: bool,
    pub safety_flag: bool,
    pub environmental_flag: bool,
    pub quality_flag: bool,
    // Priority
    pub reported_urgency: String,
    pub validated_urgency: Option<String>,
    // Timing
    pub observed_at: Option<String>,
    pub submitted_at: String,
    // Review / approval tracking
    pub review_team_id: Option<i64>,
    pub reviewer_id: Option<i64>,
    pub screened_at: Option<String>,
    pub approved_at: Option<String>,
    pub deferred_until: Option<String>,
    pub declined_at: Option<String>,
    pub closed_at: Option<String>,
    pub archived_at: Option<String>,
    // WO linkage
    pub converted_to_wo_id: Option<i64>,
    pub converted_at: Option<String>,
    // Immutable SLA snapshot (frozen at create / backfill — never rewritten on rule edit)
    pub sla_rule_id: Option<i64>,
    pub sla_target_response_hours: Option<i64>,
    pub sla_target_resolution_hours: Option<i64>,
    pub sla_escalation_threshold_hours: Option<i64>,
    pub sla_response_deadline: Option<String>,
    pub sla_resolution_deadline: Option<String>,
    pub sla_response_breach_notified_at: Option<String>,
    pub sla_resolution_breach_notified_at: Option<String>,
    // Review decision fields
    pub reviewer_note: Option<String>,
    pub classification_code_id: Option<i64>,
    // Recurrence
    pub is_recurrence_flag: bool,
    pub recurrence_di_id: Option<i64>,
    pub source_inspection_anomaly_id: Option<i64>,
    // Disposition / lifecycle (migration 139)
    pub disposition_code: Option<String>,
    pub disposition_notes: Option<String>,
    pub related_di_id: Option<i64>,
    pub closed_by_id: Option<i64>,
    pub deferred_from_status: Option<String>,
    // Concurrency
    pub row_version: i64,
    // Metadata
    pub submitter_id: i64,
    pub created_at: String,
    pub updated_at: String,
    // Display enrichment (JOINs — never render FKs in UI)
    pub asset_code: Option<String>,
    pub asset_label: Option<String>,
    pub org_node_code: Option<String>,
    pub org_node_label: Option<String>,
    pub submitter_display_name: Option<String>,
    pub reviewer_display_name: Option<String>,
    pub converted_to_wo_code: Option<String>,
    pub converted_to_wo_title: Option<String>,
    pub related_di_code: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiTransitionInput — command payload for state transitions
// ═══════════════════════════════════════════════════════════════════════════════

/// Input for requesting a DI state transition.
#[derive(Debug, Clone, Deserialize)]
pub struct DiTransitionInput {
    pub di_id: i64,
    pub to_status: String,
    pub actor_id: i64,
    pub reason_code: Option<String>,
    pub notes: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping helpers
// ═══════════════════════════════════════════════════════════════════════════════

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "intervention_requests row decode failed for column '{column}': {e}"
    ))
}

/// Map a sea-orm `QueryResult` row to an `InterventionRequest`.
pub fn map_intervention_request(row: &QueryResult) -> AppResult<InterventionRequest> {
    Ok(InterventionRequest {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        code: row.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?,
        asset_id: row
            .try_get::<i64>("", "asset_id")
            .map_err(|e| decode_err("asset_id", e))?,
        sub_asset_ref: row
            .try_get::<Option<String>>("", "sub_asset_ref")
            .map_err(|e| decode_err("sub_asset_ref", e))?,
        org_node_id: row
            .try_get::<i64>("", "org_node_id")
            .map_err(|e| decode_err("org_node_id", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        title: row.try_get::<String>("", "title").map_err(|e| decode_err("title", e))?,
        description: row
            .try_get::<String>("", "description")
            .map_err(|e| decode_err("description", e))?,
        origin_type: row
            .try_get::<String>("", "origin_type")
            .map_err(|e| decode_err("origin_type", e))?,
        request_type: row
            .try_get::<String>("", "request_type")
            .map_err(|e| decode_err("request_type", e))?,
        symptom_code_id: row
            .try_get::<Option<i64>>("", "symptom_code_id")
            .map_err(|e| decode_err("symptom_code_id", e))?,
        impact_level: row
            .try_get::<String>("", "impact_level")
            .map_err(|e| decode_err("impact_level", e))?,
        production_impact: i64_to_bool(
            row.try_get::<i64>("", "production_impact")
                .map_err(|e| decode_err("production_impact", e))?,
        ),
        safety_flag: i64_to_bool(
            row.try_get::<i64>("", "safety_flag")
                .map_err(|e| decode_err("safety_flag", e))?,
        ),
        environmental_flag: i64_to_bool(
            row.try_get::<i64>("", "environmental_flag")
                .map_err(|e| decode_err("environmental_flag", e))?,
        ),
        quality_flag: i64_to_bool(
            row.try_get::<i64>("", "quality_flag")
                .map_err(|e| decode_err("quality_flag", e))?,
        ),
        reported_urgency: row
            .try_get::<String>("", "reported_urgency")
            .map_err(|e| decode_err("reported_urgency", e))?,
        validated_urgency: row
            .try_get::<Option<String>>("", "validated_urgency")
            .map_err(|e| decode_err("validated_urgency", e))?,
        observed_at: row
            .try_get::<Option<String>>("", "observed_at")
            .map_err(|e| decode_err("observed_at", e))?,
        submitted_at: row
            .try_get::<String>("", "submitted_at")
            .map_err(|e| decode_err("submitted_at", e))?,
        review_team_id: row
            .try_get::<Option<i64>>("", "review_team_id")
            .map_err(|e| decode_err("review_team_id", e))?,
        reviewer_id: row
            .try_get::<Option<i64>>("", "reviewer_id")
            .map_err(|e| decode_err("reviewer_id", e))?,
        screened_at: row
            .try_get::<Option<String>>("", "screened_at")
            .map_err(|e| decode_err("screened_at", e))?,
        approved_at: row
            .try_get::<Option<String>>("", "approved_at")
            .map_err(|e| decode_err("approved_at", e))?,
        deferred_until: row
            .try_get::<Option<String>>("", "deferred_until")
            .map_err(|e| decode_err("deferred_until", e))?,
        declined_at: row
            .try_get::<Option<String>>("", "declined_at")
            .map_err(|e| decode_err("declined_at", e))?,
        closed_at: row
            .try_get::<Option<String>>("", "closed_at")
            .map_err(|e| decode_err("closed_at", e))?,
        archived_at: row
            .try_get::<Option<String>>("", "archived_at")
            .map_err(|e| decode_err("archived_at", e))?,
        converted_to_wo_id: row
            .try_get::<Option<i64>>("", "converted_to_wo_id")
            .map_err(|e| decode_err("converted_to_wo_id", e))?,
        converted_at: row
            .try_get::<Option<String>>("", "converted_at")
            .map_err(|e| decode_err("converted_at", e))?,
        sla_rule_id: row
            .try_get::<Option<i64>>("", "sla_rule_id")
            .map_err(|e| decode_err("sla_rule_id", e))?,
        sla_target_response_hours: row
            .try_get::<Option<i64>>("", "sla_target_response_hours")
            .map_err(|e| decode_err("sla_target_response_hours", e))?,
        sla_target_resolution_hours: row
            .try_get::<Option<i64>>("", "sla_target_resolution_hours")
            .map_err(|e| decode_err("sla_target_resolution_hours", e))?,
        sla_escalation_threshold_hours: row
            .try_get::<Option<i64>>("", "sla_escalation_threshold_hours")
            .map_err(|e| decode_err("sla_escalation_threshold_hours", e))?,
        sla_response_deadline: row
            .try_get::<Option<String>>("", "sla_response_deadline")
            .map_err(|e| decode_err("sla_response_deadline", e))?,
        sla_resolution_deadline: row
            .try_get::<Option<String>>("", "sla_resolution_deadline")
            .map_err(|e| decode_err("sla_resolution_deadline", e))?,
        sla_response_breach_notified_at: row
            .try_get::<Option<String>>("", "sla_response_breach_notified_at")
            .map_err(|e| decode_err("sla_response_breach_notified_at", e))?,
        sla_resolution_breach_notified_at: row
            .try_get::<Option<String>>("", "sla_resolution_breach_notified_at")
            .map_err(|e| decode_err("sla_resolution_breach_notified_at", e))?,
        reviewer_note: row
            .try_get::<Option<String>>("", "reviewer_note")
            .map_err(|e| decode_err("reviewer_note", e))?,
        classification_code_id: row
            .try_get::<Option<i64>>("", "classification_code_id")
            .map_err(|e| decode_err("classification_code_id", e))?,
        is_recurrence_flag: i64_to_bool(
            row.try_get::<i64>("", "is_recurrence_flag")
                .map_err(|e| decode_err("is_recurrence_flag", e))?,
        ),
        recurrence_di_id: row
            .try_get::<Option<i64>>("", "recurrence_di_id")
            .map_err(|e| decode_err("recurrence_di_id", e))?,
        source_inspection_anomaly_id: row
            .try_get::<Option<i64>>("", "source_inspection_anomaly_id")
            .map_err(|e| decode_err("source_inspection_anomaly_id", e))?,
        // New fields added by migration 139 — use .ok().flatten() for backward compat
        disposition_code: row.try_get::<Option<String>>("", "disposition_code").ok().flatten(),
        disposition_notes: row.try_get::<Option<String>>("", "disposition_notes").ok().flatten(),
        related_di_id: row.try_get::<Option<i64>>("", "related_di_id").ok().flatten(),
        closed_by_id: row.try_get::<Option<i64>>("", "closed_by_id").ok().flatten(),
        deferred_from_status: row.try_get::<Option<String>>("", "deferred_from_status").ok().flatten(),
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        submitter_id: row
            .try_get::<i64>("", "submitter_id")
            .map_err(|e| decode_err("submitter_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
        asset_code: row
            .try_get::<Option<String>>("", "asset_code")
            .map_err(|e| decode_err("asset_code", e))?,
        asset_label: row
            .try_get::<Option<String>>("", "asset_label")
            .map_err(|e| decode_err("asset_label", e))?,
        org_node_code: row
            .try_get::<Option<String>>("", "org_node_code")
            .map_err(|e| decode_err("org_node_code", e))?,
        org_node_label: row
            .try_get::<Option<String>>("", "org_node_label")
            .map_err(|e| decode_err("org_node_label", e))?,
        submitter_display_name: row
            .try_get::<Option<String>>("", "submitter_display_name")
            .map_err(|e| decode_err("submitter_display_name", e))?,
        reviewer_display_name: row
            .try_get::<Option<String>>("", "reviewer_display_name")
            .map_err(|e| decode_err("reviewer_display_name", e))?,
        converted_to_wo_code: row
            .try_get::<Option<String>>("", "converted_to_wo_code")
            .map_err(|e| decode_err("converted_to_wo_code", e))?,
        converted_to_wo_title: row
            .try_get::<Option<String>>("", "converted_to_wo_title")
            .map_err(|e| decode_err("converted_to_wo_title", e))?,
        related_di_code: row.try_get::<Option<String>>("", "related_di_code").ok().flatten(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// State machine guard
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that transitioning from `from` to `to` is allowed by the PRD §6.4
/// transition table. Returns `Err` with a descriptive message on illegal moves.
pub fn guard_transition(from: &DiStatus, to: &DiStatus) -> Result<(), String> {
    if from.allowed_transitions().contains(to) {
        Ok(())
    } else {
        Err(format!(
            "Illegal DI state transition: '{}' -> '{}'",
            from.as_str(),
            to.as_str()
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DI code generator
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate the next unique DI code in the format `DI-NNNN`.
/// Reads the current max sequence from the database and increments.
/// The code is never recycled after deletion or archival.
pub async fn generate_di_code(db: &DatabaseConnection) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(CAST(SUBSTR(code, 4) AS INTEGER)), 0) + 1 AS next_seq \
             FROM intervention_requests WHERE code LIKE 'DI-%'"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("DI code sequence query returned no rows")))?;

    let next_seq: i64 = row
        .try_get::<i64>("", "next_seq")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("DI code decode error: {e}")))?;

    Ok(format!("DI-{next_seq:04}"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_status_round_trip() {
        let all = [
            DiStatus::Submitted,
            DiStatus::InReview,
            DiStatus::ReturnedForClarification,
            DiStatus::AwaitingApproval,
            DiStatus::Approved,
            DiStatus::Deferred,
            DiStatus::Closed,
        ];
        assert_eq!(all.len(), 7);

        for status in &all {
            let s = status.as_str();
            let parsed = DiStatus::try_from_str(s).unwrap();
            assert_eq!(*status, parsed, "Round-trip failed for '{s}'");
        }
    }

    #[test]
    fn test_invalid_status_rejected() {
        assert!(DiStatus::try_from_str("invalid").is_err());
        assert!(DiStatus::try_from_str("pending_review").is_err());
        assert!(DiStatus::try_from_str("rejected").is_err());
        assert!(DiStatus::try_from_str("").is_err());
    }

    #[test]
    fn test_all_valid_forward_transitions() {
        let cases = [
            (DiStatus::Submitted, DiStatus::InReview),
            (DiStatus::Submitted, DiStatus::Closed),
            (DiStatus::InReview, DiStatus::ReturnedForClarification),
            (DiStatus::InReview, DiStatus::AwaitingApproval),
            (DiStatus::InReview, DiStatus::Closed),
            (DiStatus::InReview, DiStatus::Deferred),
            (DiStatus::ReturnedForClarification, DiStatus::InReview),
            (DiStatus::ReturnedForClarification, DiStatus::Closed),
            (DiStatus::AwaitingApproval, DiStatus::Approved),
            (DiStatus::AwaitingApproval, DiStatus::Closed),
            (DiStatus::AwaitingApproval, DiStatus::Deferred),
            (DiStatus::Approved, DiStatus::Closed),
            (DiStatus::Approved, DiStatus::Deferred),
            (DiStatus::Deferred, DiStatus::InReview),
            (DiStatus::Deferred, DiStatus::AwaitingApproval),
            (DiStatus::Deferred, DiStatus::Approved),
        ];

        for (from, to) in &cases {
            assert!(
                guard_transition(from, to).is_ok(),
                "Expected valid transition: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[test]
    fn test_invalid_transitions_rejected() {
        let invalid_cases = [
            (DiStatus::Submitted, DiStatus::Approved),
            (DiStatus::Closed, DiStatus::Submitted),
            (DiStatus::Closed, DiStatus::InReview),
            (DiStatus::Approved, DiStatus::InReview),
            (DiStatus::AwaitingApproval, DiStatus::InReview),
        ];

        for (from, to) in &invalid_cases {
            assert!(
                guard_transition(from, to).is_err(),
                "Expected invalid transition: {} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }

    #[test]
    fn test_closed_has_no_outbound_transitions() {
        assert!(DiStatus::Closed.allowed_transitions().is_empty());
    }

    #[test]
    fn test_immutable_states() {
        assert!(DiStatus::Closed.is_immutable());
        for s in [
            DiStatus::Submitted,
            DiStatus::InReview,
            DiStatus::ReturnedForClarification,
            DiStatus::AwaitingApproval,
            DiStatus::Approved,
            DiStatus::Deferred,
        ] {
            assert!(!s.is_immutable(), "{} should be mutable", s.as_str());
        }
    }

    #[test]
    fn test_step_up_states() {
        assert!(DiStatus::Approved.requires_step_up_to_enter());
        for s in [
            DiStatus::Submitted,
            DiStatus::InReview,
            DiStatus::ReturnedForClarification,
            DiStatus::AwaitingApproval,
            DiStatus::Deferred,
            DiStatus::Closed,
        ] {
            assert!(
                !s.requires_step_up_to_enter(),
                "{} should not require step-up to enter",
                s.as_str()
            );
        }
    }

    #[test]
    fn test_origin_type_round_trip() {
        let all = [
            DiOriginType::Operator,
            DiOriginType::Technician,
            DiOriginType::Inspection,
            DiOriginType::Pm,
            DiOriginType::Iot,
            DiOriginType::Quality,
            DiOriginType::Hse,
            DiOriginType::Production,
            DiOriginType::External,
        ];
        for t in &all {
            let s = t.as_str();
            let parsed = DiOriginType::try_from_str(s).unwrap();
            assert_eq!(*t, parsed);
        }
    }

    #[test]
    fn test_urgency_round_trip() {
        let all = [DiUrgency::Low, DiUrgency::Medium, DiUrgency::High, DiUrgency::Critical];
        for u in &all {
            let s = u.as_str();
            let parsed = DiUrgency::try_from_str(s).unwrap();
            assert_eq!(*u, parsed);
        }
    }

    #[test]
    fn test_impact_level_round_trip() {
        let all = [
            DiImpactLevel::Unknown,
            DiImpactLevel::None,
            DiImpactLevel::Minor,
            DiImpactLevel::Major,
            DiImpactLevel::Critical,
        ];
        for l in &all {
            let s = l.as_str();
            let parsed = DiImpactLevel::try_from_str(s).unwrap();
            assert_eq!(*l, parsed);
        }
    }
}

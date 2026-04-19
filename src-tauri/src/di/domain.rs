//! DI domain types, state machine, and code generation.
//!
//! Phase 2 - Sub-phase 04 - File 01 - Sprint S1.
//!
//! Implements the full 11-state PRD §6.4 workflow:
//!   Submitted → Pending Review → Returned for Clarification → Rejected →
//!   Screened → Awaiting Approval → Approved for Planning → Deferred →
//!   Converted to Work Order → Closed as Non-Executable → Archived
//!
//! The state machine is enforced in Rust — the frontend never decides validity
//! of a transition. `guard_transition` is the single authority.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// DiStatus — PRD §6.4 exact 11-state enum
// ═══════════════════════════════════════════════════════════════════════════════

/// All legal states for an intervention request, per PRD §6.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiStatus {
    Submitted,
    PendingReview,
    ReturnedForClarification,
    Rejected,
    Screened,
    AwaitingApproval,
    ApprovedForPlanning,
    Deferred,
    ConvertedToWorkOrder,
    ClosedAsNonExecutable,
    Archived,
}

impl DiStatus {
    /// Database-persisted snake_case representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::PendingReview => "pending_review",
            Self::ReturnedForClarification => "returned_for_clarification",
            Self::Rejected => "rejected",
            Self::Screened => "screened",
            Self::AwaitingApproval => "awaiting_approval",
            Self::ApprovedForPlanning => "approved_for_planning",
            Self::Deferred => "deferred",
            Self::ConvertedToWorkOrder => "converted_to_work_order",
            Self::ClosedAsNonExecutable => "closed_as_non_executable",
            Self::Archived => "archived",
        }
    }

    /// Parse from the DB-stored snake_case string.
    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "submitted" => Ok(Self::Submitted),
            "pending_review" => Ok(Self::PendingReview),
            "returned_for_clarification" => Ok(Self::ReturnedForClarification),
            "rejected" => Ok(Self::Rejected),
            "screened" => Ok(Self::Screened),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved_for_planning" => Ok(Self::ApprovedForPlanning),
            "deferred" => Ok(Self::Deferred),
            "converted_to_work_order" => Ok(Self::ConvertedToWorkOrder),
            "closed_as_non_executable" => Ok(Self::ClosedAsNonExecutable),
            "archived" => Ok(Self::Archived),
            other => Err(format!("Unknown DI status: '{other}'")),
        }
    }

    /// Exact PRD §6.4 transition table. No additions, no omissions.
    pub fn allowed_transitions(&self) -> &'static [DiStatus] {
        match self {
            Self::Submitted => &[Self::PendingReview],
            Self::PendingReview => &[
                Self::Screened,
                Self::ReturnedForClarification,
                Self::Rejected,
            ],
            Self::ReturnedForClarification => &[Self::PendingReview],
            Self::Screened => &[Self::AwaitingApproval, Self::Rejected],
            Self::AwaitingApproval => &[
                Self::ApprovedForPlanning,
                Self::Deferred,
                Self::Rejected,
            ],
            Self::ApprovedForPlanning => &[
                Self::ConvertedToWorkOrder,
                Self::Deferred,
                Self::ClosedAsNonExecutable,
            ],
            Self::Deferred => &[Self::AwaitingApproval],
            Self::ConvertedToWorkOrder => &[Self::Archived],
            Self::ClosedAsNonExecutable => &[Self::Archived],
            Self::Rejected => &[Self::Archived],
            Self::Archived => &[],
        }
    }

    /// Terminal evidence states that lock the DI from field edits.
    /// Only commentary and attachments are allowed after these states.
    pub fn is_immutable_after_conversion(&self) -> bool {
        matches!(
            self,
            Self::ConvertedToWorkOrder
                | Self::ClosedAsNonExecutable
                | Self::Rejected
                | Self::Archived
        )
    }

    /// States whose transition actions require step-up reauthentication.
    pub fn requires_step_up(&self) -> bool {
        matches!(
            self,
            Self::ApprovedForPlanning | Self::ConvertedToWorkOrder
        )
    }
}

impl std::fmt::Display for DiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DiOriginType — PRD §6.4 intake origin classification
// ═══════════════════════════════════════════════════════════════════════════════

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
    // Review decision fields
    pub reviewer_note: Option<String>,
    pub classification_code_id: Option<i64>,
    // Recurrence
    pub is_recurrence_flag: bool,
    pub recurrence_di_id: Option<i64>,
    pub source_inspection_anomaly_id: Option<i64>,
    // Concurrency
    pub row_version: i64,
    // Metadata
    pub submitter_id: i64,
    pub created_at: String,
    pub updated_at: String,
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
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
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
        title: row
            .try_get::<String>("", "title")
            .map_err(|e| decode_err("title", e))?,
        description: row
            .try_get::<String>("", "description")
            .map_err(|e| decode_err("description", e))?,
        origin_type: row
            .try_get::<String>("", "origin_type")
            .map_err(|e| decode_err("origin_type", e))?,
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
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "DI code sequence query returned no rows"
            ))
        })?;

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

    // ── Status string round-trip ──────────────────────────────────────────

    #[test]
    fn test_all_status_round_trip() {
        let all = [
            DiStatus::Submitted,
            DiStatus::PendingReview,
            DiStatus::ReturnedForClarification,
            DiStatus::Rejected,
            DiStatus::Screened,
            DiStatus::AwaitingApproval,
            DiStatus::ApprovedForPlanning,
            DiStatus::Deferred,
            DiStatus::ConvertedToWorkOrder,
            DiStatus::ClosedAsNonExecutable,
            DiStatus::Archived,
        ];
        assert_eq!(all.len(), 11, "PRD §6.4 requires exactly 11 states");

        for status in &all {
            let s = status.as_str();
            let parsed = DiStatus::try_from_str(s).unwrap();
            assert_eq!(*status, parsed, "Round-trip failed for '{s}'");
        }
    }

    #[test]
    fn test_invalid_status_rejected() {
        assert!(DiStatus::try_from_str("invalid").is_err());
        assert!(DiStatus::try_from_str("").is_err());
        assert!(DiStatus::try_from_str("SUBMITTED").is_err()); // case-sensitive
    }

    // ── Transition table coverage ─────────────────────────────────────────

    #[test]
    fn test_all_valid_forward_transitions() {
        // Every listed transition must pass guard_transition.
        let cases = [
            (DiStatus::Submitted, DiStatus::PendingReview),
            (DiStatus::PendingReview, DiStatus::Screened),
            (DiStatus::PendingReview, DiStatus::ReturnedForClarification),
            (DiStatus::PendingReview, DiStatus::Rejected),
            (DiStatus::ReturnedForClarification, DiStatus::PendingReview),
            (DiStatus::Screened, DiStatus::AwaitingApproval),
            (DiStatus::Screened, DiStatus::Rejected),
            (DiStatus::AwaitingApproval, DiStatus::ApprovedForPlanning),
            (DiStatus::AwaitingApproval, DiStatus::Deferred),
            (DiStatus::AwaitingApproval, DiStatus::Rejected),
            (DiStatus::ApprovedForPlanning, DiStatus::ConvertedToWorkOrder),
            (DiStatus::ApprovedForPlanning, DiStatus::Deferred),
            (DiStatus::ApprovedForPlanning, DiStatus::ClosedAsNonExecutable),
            (DiStatus::Deferred, DiStatus::AwaitingApproval),
            (DiStatus::ConvertedToWorkOrder, DiStatus::Archived),
            (DiStatus::ClosedAsNonExecutable, DiStatus::Archived),
            (DiStatus::Rejected, DiStatus::Archived),
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
            (DiStatus::Submitted, DiStatus::ApprovedForPlanning),
            (DiStatus::Submitted, DiStatus::Archived),
            (DiStatus::Archived, DiStatus::Submitted),
            (DiStatus::Rejected, DiStatus::Submitted),
            (DiStatus::ConvertedToWorkOrder, DiStatus::Submitted),
            (DiStatus::Screened, DiStatus::Submitted),
            (DiStatus::Deferred, DiStatus::ApprovedForPlanning),
            (DiStatus::ClosedAsNonExecutable, DiStatus::Submitted),
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
    fn test_archived_has_no_outbound_transitions() {
        assert!(DiStatus::Archived.allowed_transitions().is_empty());
    }

    #[test]
    fn test_converted_to_wo_only_goes_to_archived() {
        let allowed = DiStatus::ConvertedToWorkOrder.allowed_transitions();
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0], DiStatus::Archived);
    }

    // ── Immutability flag ─────────────────────────────────────────────────

    #[test]
    fn test_immutable_states() {
        let immutable = [
            DiStatus::ConvertedToWorkOrder,
            DiStatus::ClosedAsNonExecutable,
            DiStatus::Rejected,
            DiStatus::Archived,
        ];
        for s in &immutable {
            assert!(
                s.is_immutable_after_conversion(),
                "{} should be immutable",
                s.as_str()
            );
        }

        let mutable = [
            DiStatus::Submitted,
            DiStatus::PendingReview,
            DiStatus::ReturnedForClarification,
            DiStatus::Screened,
            DiStatus::AwaitingApproval,
            DiStatus::ApprovedForPlanning,
            DiStatus::Deferred,
        ];
        for s in &mutable {
            assert!(
                !s.is_immutable_after_conversion(),
                "{} should be mutable",
                s.as_str()
            );
        }
    }

    // ── Step-up requirement ───────────────────────────────────────────────

    #[test]
    fn test_step_up_states() {
        assert!(DiStatus::ApprovedForPlanning.requires_step_up());
        assert!(DiStatus::ConvertedToWorkOrder.requires_step_up());

        // All others should not require step-up
        let no_step_up = [
            DiStatus::Submitted,
            DiStatus::PendingReview,
            DiStatus::ReturnedForClarification,
            DiStatus::Rejected,
            DiStatus::Screened,
            DiStatus::AwaitingApproval,
            DiStatus::Deferred,
            DiStatus::ClosedAsNonExecutable,
            DiStatus::Archived,
        ];
        for s in &no_step_up {
            assert!(
                !s.requires_step_up(),
                "{} should not require step-up",
                s.as_str()
            );
        }
    }

    // ── Origin type round-trip ────────────────────────────────────────────

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

    // ── Urgency round-trip ────────────────────────────────────────────────

    #[test]
    fn test_urgency_round_trip() {
        let all = [
            DiUrgency::Low,
            DiUrgency::Medium,
            DiUrgency::High,
            DiUrgency::Critical,
        ];
        for u in &all {
            let s = u.as_str();
            let parsed = DiUrgency::try_from_str(s).unwrap();
            assert_eq!(*u, parsed);
        }
    }

    // ── Impact level round-trip ───────────────────────────────────────────

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

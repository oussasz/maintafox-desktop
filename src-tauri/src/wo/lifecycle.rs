//! PRD §6.5 work-order lifecycle (12 states), derived — never stored (Phase 6.4, option B).
//!
//! The app stores 8 phases (`draft`, `planning`, `ready`, `in_progress`, `on_hold`, `completed`,
//! `closed`, `cancelled`; migration 130). The PRD's 12-state lifecycle is a pure function of facts
//! held on the work order row and its own delay segments — never of tenant settings or other
//! modules' live data — so a WO's lifecycle state only changes when that WO is edited, and no
//! state is ever written or synchronised by itself:
//!
//! | phase         | facts                                                     | lifecycle state           |
//! |---------------|-----------------------------------------------------------|---------------------------|
//! | draft         | —                                                         | draft                     |
//! | planning      | approval required for this WO and not approved yet        | awaiting_approval         |
//! | planning      | otherwise                                                 | planned                   |
//! | ready         | schedule (planned start + end) and assignee (person/team) | assigned                  |
//! | ready         | otherwise                                                 | ready_to_schedule         |
//! | in_progress   | —                                                         | in_progress               |
//! | on_hold       | the open hold is a prerequisite hold                      | waiting_for_prerequisite  |
//! | on_hold       | otherwise (pause, or no open segment)                     | paused                    |
//! | completed     | technically verified                                      | technically_verified      |
//! | completed     | otherwise                                                 | mechanically_complete     |
//! | closed        | —                                                         | closed                    |
//! | cancelled     | —                                                         | cancelled                 |
//!
//! `planning_approval_required` is a fact of the WO, copied from the tenant setting when the WO
//! enters planning (`workflow::transition::log_transition`); changing the setting later only
//! affects WOs that enter planning afterwards. `hold_kind` is written by the hold action
//! (`pause` / `prerequisite`) on the delay segment it opens.
//!
//! [`derive`] is the single source of truth. [`SQL_CASE`] (list filters, analytics) and the
//! TypeScript `deriveWoLifecycle` (display) mirror it; both are checked against `derive` for every
//! combination of facts (`lifecycle_tests.rs`, `shared/wo/lifecycle-cases.json`).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::wo::workflow::state_machine::WoStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
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

impl LifecycleState {
    /// PRD order.
    pub const ALL: [LifecycleState; 12] = [
        Self::Draft,
        Self::AwaitingApproval,
        Self::Planned,
        Self::ReadyToSchedule,
        Self::Assigned,
        Self::WaitingForPrerequisite,
        Self::InProgress,
        Self::Paused,
        Self::MechanicallyComplete,
        Self::TechnicallyVerified,
        Self::Closed,
        Self::Cancelled,
    ];

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

    pub fn try_from_str(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Kind of hold recorded on a delay segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldKind {
    Pause,
    Prerequisite,
}

impl HoldKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Prerequisite => "prerequisite",
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "pause" => Some(Self::Pause),
            "prerequisite" => Some(Self::Prerequisite),
            _ => None,
        }
    }
}

/// The facts the lifecycle state depends on (all on the WO row or its delay segments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleFacts {
    pub phase: WoStatus,
    pub planning_approval_required: bool,
    pub planning_approved: bool,
    /// Planned start and planned end both set.
    pub has_schedule: bool,
    /// Primary responsible or assigned team set.
    pub has_assignee: bool,
    pub technically_verified: bool,
    /// Kind of the open (not ended) delay segment, if any.
    pub open_hold: Option<HoldKind>,
}

/// The PRD lifecycle state of a work order. Total: every combination of facts gives one state.
pub fn derive(f: &LifecycleFacts) -> LifecycleState {
    use LifecycleState as L;
    match f.phase {
        WoStatus::Draft => L::Draft,
        WoStatus::Planning if f.planning_approval_required && !f.planning_approved => L::AwaitingApproval,
        WoStatus::Planning => L::Planned,
        WoStatus::Ready if f.has_schedule && f.has_assignee => L::Assigned,
        WoStatus::Ready => L::ReadyToSchedule,
        WoStatus::InProgress => L::InProgress,
        WoStatus::OnHold if f.open_hold == Some(HoldKind::Prerequisite) => L::WaitingForPrerequisite,
        WoStatus::OnHold => L::Paused,
        WoStatus::Completed if f.technically_verified => L::TechnicallyVerified,
        WoStatus::Completed => L::MechanicallyComplete,
        WoStatus::Closed => L::Closed,
        WoStatus::Cancelled => L::Cancelled,
    }
}

/// SQL mirror of [`derive`] over `work_orders wo` joined to `work_order_statuses wos`. Kept equal
/// to `derive` by `lifecycle_tests::sql_case_matches_derive_for_every_combination`.
pub const SQL_CASE: &str = "(CASE \
    WHEN wos.code = 'draft' THEN 'draft' \
    WHEN wos.code = 'planning' AND wo.planning_approval_required = 1 AND wo.planning_approved_at IS NULL \
         THEN 'awaiting_approval' \
    WHEN wos.code = 'planning' THEN 'planned' \
    WHEN wos.code = 'ready' AND wo.planned_start IS NOT NULL AND TRIM(wo.planned_start) <> '' \
         AND wo.planned_end IS NOT NULL AND TRIM(wo.planned_end) <> '' \
         AND (wo.primary_responsible_id IS NOT NULL OR wo.assigned_team_id IS NOT NULL) THEN 'assigned' \
    WHEN wos.code = 'ready' THEN 'ready_to_schedule' \
    WHEN wos.code = 'in_progress' THEN 'in_progress' \
    WHEN wos.code = 'on_hold' AND (SELECT s.hold_kind FROM work_order_delay_segments s \
         WHERE s.work_order_id = wo.id AND s.ended_at IS NULL ORDER BY s.id DESC LIMIT 1) = 'prerequisite' \
         THEN 'waiting_for_prerequisite' \
    WHEN wos.code = 'on_hold' THEN 'paused' \
    WHEN wos.code = 'completed' AND wo.technically_verified_at IS NOT NULL THEN 'technically_verified' \
    WHEN wos.code = 'completed' THEN 'mechanically_complete' \
    WHEN wos.code = 'closed' THEN 'closed' \
    WHEN wos.code = 'cancelled' THEN 'cancelled' \
    END)";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("wo lifecycle decode '{field}': {err}"))
}

fn non_blank(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| !s.trim().is_empty())
}

/// Facts of one work order, read in the caller's transaction.
pub async fn load_facts(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<LifecycleFacts> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS phase, wo.planning_approval_required, wo.planning_approved_at, \
                    wo.planned_start, wo.planned_end, wo.primary_responsible_id, wo.assigned_team_id, \
                    wo.technically_verified_at, \
                    (SELECT s.hold_kind FROM work_order_delay_segments s \
                     WHERE s.work_order_id = wo.id AND s.ended_at IS NULL ORDER BY s.id DESC LIMIT 1) AS open_hold \
             FROM work_orders wo JOIN work_order_statuses wos ON wos.id = wo.status_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let phase: String = row.try_get("", "phase").map_err(|e| decode_err("phase", e))?;
    let required: i64 = row
        .try_get("", "planning_approval_required")
        .map_err(|e| decode_err("planning_approval_required", e))?;
    let approved: Option<String> = row
        .try_get("", "planning_approved_at")
        .map_err(|e| decode_err("planning_approved_at", e))?;
    let start: Option<String> = row
        .try_get("", "planned_start")
        .map_err(|e| decode_err("planned_start", e))?;
    let end: Option<String> = row
        .try_get("", "planned_end")
        .map_err(|e| decode_err("planned_end", e))?;
    let person: Option<i64> = row
        .try_get("", "primary_responsible_id")
        .map_err(|e| decode_err("primary_responsible_id", e))?;
    let team: Option<i64> = row
        .try_get("", "assigned_team_id")
        .map_err(|e| decode_err("assigned_team_id", e))?;
    let verified: Option<String> = row
        .try_get("", "technically_verified_at")
        .map_err(|e| decode_err("technically_verified_at", e))?;
    let hold: Option<String> = row.try_get("", "open_hold").map_err(|e| decode_err("open_hold", e))?;
    Ok(LifecycleFacts {
        phase: WoStatus::try_from_str(&phase).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?,
        planning_approval_required: required != 0,
        planning_approved: approved.is_some(),
        has_schedule: non_blank(&start) && non_blank(&end),
        has_assignee: person.is_some() || team.is_some(),
        technically_verified: verified.is_some(),
        open_hold: match hold.as_deref() {
            None => None,
            Some(k) => Some(
                HoldKind::try_from_str(k)
                    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("unknown hold_kind '{k}'")))?,
            ),
        },
    })
}

pub async fn lifecycle_of(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<LifecycleState> {
    Ok(derive(&load_facts(conn, wo_id).await?))
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

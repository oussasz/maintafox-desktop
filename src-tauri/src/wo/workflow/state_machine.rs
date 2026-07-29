//! WO status machine + actionability matrix.

use serde::{Deserialize, Serialize};

/// Canonical WO lifecycle statuses (Option B).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WoStatus {
    Draft,
    Planning,
    Ready,
    InProgress,
    OnHold,
    Completed,
    Closed,
    Cancelled,
}

impl WoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Planning => "planning",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::OnHold => "on_hold",
            Self::Completed => "completed",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn try_from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(Self::Draft),
            "planning" => Ok(Self::Planning),
            "ready" => Ok(Self::Ready),
            "in_progress" => Ok(Self::InProgress),
            "on_hold" => Ok(Self::OnHold),
            "completed" => Ok(Self::Completed),
            "closed" => Ok(Self::Closed),
            "cancelled" => Ok(Self::Cancelled),
            // Legacy aliases (read-only parse for old logs / mid-migration rows)
            "awaiting_approval" | "planned" | "ready_to_schedule" | "assigned" => {
                Ok(Self::Planning)
            }
            "waiting_for_prerequisite" | "paused" => Ok(Self::OnHold),
            "mechanically_complete" | "technically_verified" => Ok(Self::Completed),
            other => Err(format!("Unknown WO status: '{other}'")),
        }
    }

    pub fn allowed_transitions(&self) -> &'static [WoStatus] {
        match self {
            Self::Draft => &[Self::Planning, Self::Cancelled],
            Self::Planning => &[Self::Ready, Self::Cancelled],
            Self::Ready => &[Self::Planning, Self::InProgress, Self::Cancelled],
            Self::InProgress => &[Self::OnHold, Self::Completed, Self::Cancelled],
            Self::OnHold => &[Self::InProgress, Self::Cancelled],
            Self::Completed => &[Self::InProgress, Self::Planning, Self::Closed, Self::Cancelled],
            Self::Closed => &[],
            Self::Cancelled => &[],
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Closed | Self::Cancelled)
    }

    pub fn is_executing(&self) -> bool {
        matches!(self, Self::InProgress | Self::OnHold)
    }

    pub fn requires_step_up_for_close(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn allowed_actions(&self) -> &'static [WoAction] {
        match self {
            Self::Draft => &[
                WoAction::EditIdentification,
                WoAction::Delete,
                WoAction::Submit,
                WoAction::Cancel,
            ],
            Self::Planning => &[
                WoAction::EditPlan,
                WoAction::EditSchedule,
                WoAction::EditAssignment,
                WoAction::ReserveParts,
                WoAction::MarkReady,
                WoAction::ApprovePlanning,
                WoAction::Cancel,
            ],
            Self::Ready => &[
                WoAction::Reassign,
                WoAction::Reschedule,
                WoAction::ChangePriority,
                WoAction::Start,
                WoAction::ReturnToPlanning,
                WoAction::Cancel,
            ],
            Self::InProgress => &[
                WoAction::Hold,
                WoAction::Complete,
                WoAction::LogLabor,
                WoAction::ConsumeParts,
                WoAction::AddPhotos,
                WoAction::Cancel,
            ],
            Self::OnHold => &[WoAction::Resume, WoAction::Cancel],
            Self::Completed => &[
                WoAction::SupervisorReview,
                WoAction::Reopen,
                WoAction::Close,
                WoAction::Cancel,
            ],
            Self::Closed | Self::Cancelled => &[],
        }
    }
}

impl std::fmt::Display for WoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Domain actions (lifecycle + work). Used for actionability and event codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WoAction {
    EditIdentification,
    Delete,
    Submit,
    EditPlan,
    EditSchedule,
    EditAssignment,
    ReserveParts,
    MarkReady,
    ApprovePlanning,
    Reassign,
    Reschedule,
    ChangePriority,
    Start,
    ReturnToPlanning,
    Hold,
    Resume,
    Complete,
    LogLabor,
    ConsumeParts,
    AddPhotos,
    SupervisorReview,
    Reopen,
    Close,
    Cancel,
}

impl WoAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EditIdentification => "edit_identification",
            Self::Delete => "delete",
            Self::Submit => "submit",
            Self::EditPlan => "edit_plan",
            Self::EditSchedule => "edit_schedule",
            Self::EditAssignment => "edit_assignment",
            Self::ReserveParts => "reserve_parts",
            Self::MarkReady => "mark_ready",
            Self::ApprovePlanning => "approve_planning",
            Self::Reassign => "reassign",
            Self::Reschedule => "reschedule",
            Self::ChangePriority => "change_priority",
            Self::Start => "start",
            Self::ReturnToPlanning => "return_to_planning",
            Self::Hold => "hold",
            Self::Resume => "resume",
            Self::Complete => "complete",
            Self::LogLabor => "log_labor",
            Self::ConsumeParts => "consume_parts",
            Self::AddPhotos => "add_photos",
            Self::SupervisorReview => "supervisor_review",
            Self::Reopen => "reopen",
            Self::Close => "close",
            Self::Cancel => "cancel",
        }
    }
}

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

pub fn assert_action_allowed(status: &WoStatus, action: WoAction) -> Result<(), String> {
    if status.allowed_actions().contains(&action) {
        Ok(())
    } else {
        Err(format!(
            "Action '{}' is not allowed in status '{}'.",
            action.as_str(),
            status.as_str()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_ready_path() {
        assert!(guard_wo_transition(&WoStatus::Draft, &WoStatus::Planning).is_ok());
        assert!(guard_wo_transition(&WoStatus::Planning, &WoStatus::Ready).is_ok());
        assert!(guard_wo_transition(&WoStatus::Ready, &WoStatus::InProgress).is_ok());
        assert!(guard_wo_transition(&WoStatus::Draft, &WoStatus::Ready).is_err());
        assert!(guard_wo_transition(&WoStatus::Planning, &WoStatus::InProgress).is_err());
    }

    #[test]
    fn hold_cycle() {
        assert!(guard_wo_transition(&WoStatus::InProgress, &WoStatus::OnHold).is_ok());
        assert!(guard_wo_transition(&WoStatus::OnHold, &WoStatus::InProgress).is_ok());
    }

    #[test]
    fn reopen_targets() {
        assert!(guard_wo_transition(&WoStatus::Completed, &WoStatus::InProgress).is_ok());
        assert!(guard_wo_transition(&WoStatus::Completed, &WoStatus::Planning).is_ok());
        assert!(guard_wo_transition(&WoStatus::Completed, &WoStatus::Ready).is_err());
    }

    #[test]
    fn actionability_ready() {
        assert!(assert_action_allowed(&WoStatus::Ready, WoAction::Reassign).is_ok());
        assert!(assert_action_allowed(&WoStatus::Ready, WoAction::Start).is_ok());
        assert!(assert_action_allowed(&WoStatus::Ready, WoAction::MarkReady).is_err());
        assert!(assert_action_allowed(&WoStatus::Planning, WoAction::Start).is_err());
    }
}

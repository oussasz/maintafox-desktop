//! Phase-1 readiness rules registry.

mod approval;
mod assignment;
mod equipment;
mod parts;
mod permit;
mod planning;
mod schedule;

use crate::wo::workflow::readiness::rule::ReadinessRule;

pub fn all_ready_gate_rules() -> Vec<Box<dyn ReadinessRule>> {
    vec![
        Box::new(equipment::EquipmentAssigned),
        Box::new(planning::PlanningCompleted),
        Box::new(schedule::ScheduleDefined),
        Box::new(assignment::AssignmentCompleted),
        Box::new(permit::PermitApproved),
        Box::new(parts::PartsReserved),
        Box::new(approval::ApprovalRequired),
    ]
}

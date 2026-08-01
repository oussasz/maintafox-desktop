use crate::wo::workflow::readiness::rule::{check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct AssignmentCompleted;

impl ReadinessRule for AssignmentCompleted {
    fn code(&self) -> &'static str {
        "assignment_completed"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Assignment
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        if ctx.primary_responsible_id.is_some() || ctx.assigned_group_id.is_some() {
            check(self, ReadinessOutcome::Pass, None)
        } else {
            check(
                self,
                ReadinessOutcome::Fail,
                Some("Responsable ou équipe non assigné.".into()),
            )
        }
    }
}

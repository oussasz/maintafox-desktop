use crate::wo::workflow::readiness::rule::{
    check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity,
};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct ApprovalRequired;

impl ReadinessRule for ApprovalRequired {
    fn code(&self) -> &'static str {
        "approval_required"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Governance
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        if !ctx.approval_policy_required {
            return check(self, ReadinessOutcome::Na, None);
        }
        if ctx.planning_approved_at.is_some() {
            check(self, ReadinessOutcome::Pass, None)
        } else {
            check(
                self,
                ReadinessOutcome::Fail,
                Some("Approbation de planification requise.".into()),
            )
        }
    }
}

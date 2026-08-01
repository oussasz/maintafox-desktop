use crate::wo::workflow::readiness::rule::{
    check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity,
};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct EquipmentAssigned;

impl ReadinessRule for EquipmentAssigned {
    fn code(&self) -> &'static str {
        "equipment_assigned"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Planning
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        if ctx.equipment_id.is_some() {
            check(self, ReadinessOutcome::Pass, None)
        } else {
            check(
                self,
                ReadinessOutcome::Fail,
                Some("Équipement non assigné.".into()),
            )
        }
    }
}

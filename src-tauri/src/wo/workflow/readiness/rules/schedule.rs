use crate::wo::workflow::readiness::rule::{
    check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity,
};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct ScheduleDefined;

impl ReadinessRule for ScheduleDefined {
    fn code(&self) -> &'static str {
        "schedule_defined"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Schedule
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        match (&ctx.planned_start, &ctx.planned_end) {
            (Some(s), Some(e)) if !s.trim().is_empty() && !e.trim().is_empty() => {
                if s <= e {
                    check(self, ReadinessOutcome::Pass, None)
                } else {
                    check(
                        self,
                        ReadinessOutcome::Fail,
                        Some("La fin planifiée est antérieure au début.".into()),
                    )
                }
            }
            _ => check(
                self,
                ReadinessOutcome::Fail,
                Some("Planning (début/fin) non défini.".into()),
            ),
        }
    }
}

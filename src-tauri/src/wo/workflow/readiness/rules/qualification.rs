//! Crew qualified: the people on the work order hold what it requires (habilitations,
//! authorisations, skills — `qualification::gate`), or a supervisor recorded an override.

use crate::wo::workflow::readiness::rule::{check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct CrewQualified;

impl ReadinessRule for CrewQualified {
    fn code(&self) -> &'static str {
        "crew_qualified"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Assignment
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        match &ctx.qualification_blocker {
            None => check(self, ReadinessOutcome::Pass, None),
            Some(message) => check(self, ReadinessOutcome::Fail, Some(message.clone())),
        }
    }
}

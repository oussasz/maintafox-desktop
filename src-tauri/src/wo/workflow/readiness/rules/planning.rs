use crate::wo::workflow::readiness::rule::{check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity};
use crate::wo::workflow::readiness::WoReadinessContext;

pub struct PlanningCompleted;

impl ReadinessRule for PlanningCompleted {
    fn code(&self) -> &'static str {
        "planning_completed"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Planning
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        let title_ok = !ctx.title.trim().is_empty();
        let type_ok = ctx.type_id > 0;
        // Phase-1: if mandatory tasks exist they must be defined (count already is the defined set).
        // Rule fails when identification incomplete; mandatory tasks are informational until policy expands.
        if title_ok && type_ok {
            if ctx.mandatory_task_count < 0 {
                check(
                    self,
                    ReadinessOutcome::Fail,
                    Some("Tâches obligatoires manquantes.".into()),
                )
            } else {
                check(self, ReadinessOutcome::Pass, None)
            }
        } else {
            check(
                self,
                ReadinessOutcome::Fail,
                Some("Titre ou type d'OT manquant.".into()),
            )
        }
    }
}

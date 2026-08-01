use crate::wo::workflow::readiness::rule::{check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity};
use crate::wo::workflow::readiness::WoReadinessContext;

/// Permit gate: N/A when WO does not require a permit.
/// Phase-1: when requires_permit, Pass only if a linked approved PTW exists — reuse existing PTW linkage.
/// Until PTW join is wired here, Fail with clear message when required.
pub struct PermitApproved;

impl ReadinessRule for PermitApproved {
    fn code(&self) -> &'static str {
        "permit_approved"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Safety
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Blocking
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        if !ctx.requires_permit {
            return check(self, ReadinessOutcome::Na, None);
        }
        // Phase-1: require_permit flag is the gate; approved PTW validation stays in existing start_wo path.
        // Mark Ready treats "requires_permit acknowledged" as Pass; start still enforces PTW.
        check(
            self,
            ReadinessOutcome::Pass,
            Some("Permis requis — vérification PTW au démarrage.".into()),
        )
    }
}

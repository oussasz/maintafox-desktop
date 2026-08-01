use crate::wo::workflow::readiness::rule::{
    check, ReadinessOutcome, ReadinessRule, RuleCategory, RuleSeverity,
};
use crate::wo::workflow::readiness::WoReadinessContext;

/// Material readiness for planned WO parts (stock available / reserved).
/// Severity is Recommended (warn) — tenant policy may elevate to Blocking later.
pub struct PartsReserved;

impl ReadinessRule for PartsReserved {
    fn code(&self) -> &'static str {
        "material_readiness"
    }
    fn category(&self) -> RuleCategory {
        RuleCategory::Parts
    }
    fn severity(&self) -> RuleSeverity {
        RuleSeverity::Recommended
    }
    fn validate(&self, ctx: &WoReadinessContext) -> crate::wo::workflow::readiness::rule::ReadinessCheck {
        // No planned parts → N/A (not a materials gate).
        if ctx.planned_parts_count == 0 {
            return check(self, ReadinessOutcome::Na, None);
        }
        if ctx.materials_ready {
            return check(
                self,
                ReadinessOutcome::Pass,
                Some(format!(
                    "Materials ready ({:.0}% ready, {:.0}% reserved)",
                    ctx.materials_ready_pct, ctx.materials_reserved_pct
                )),
            );
        }
        let mut msg = format!(
            "Materials not ready: {:.0}% ready, {} missing part line(s)",
            ctx.materials_ready_pct, ctx.materials_missing_parts
        );
        if let Some(ref eta) = ctx.materials_expected_arrival {
            msg.push_str(&format!("; expected arrival {eta}"));
        }
        check(self, ReadinessOutcome::Fail, Some(msg))
    }
}

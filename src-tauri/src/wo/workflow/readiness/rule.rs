//! Readiness rule trait + metadata.

use crate::wo::workflow::readiness::WoReadinessContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCategory {
    Planning,
    Schedule,
    Assignment,
    Safety,
    Parts,
    Governance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSeverity {
    Blocking,
    Recommended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessPhase {
    ReadyGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessOutcome {
    Pass,
    Fail,
    Na,
}

#[derive(Debug, Clone)]
pub struct ReadinessCheck {
    pub code: &'static str,
    pub category: RuleCategory,
    pub severity: RuleSeverity,
    pub blocking: bool,
    pub phase: ReadinessPhase,
    pub outcome: ReadinessOutcome,
    pub message: Option<String>,
}

pub trait ReadinessRule: Send + Sync {
    fn code(&self) -> &'static str;
    fn category(&self) -> RuleCategory;
    fn severity(&self) -> RuleSeverity;
    fn blocking(&self) -> bool {
        matches!(self.severity(), RuleSeverity::Blocking)
    }
    fn phase(&self) -> ReadinessPhase {
        ReadinessPhase::ReadyGate
    }
    fn validate(&self, ctx: &WoReadinessContext) -> ReadinessCheck;
}

pub fn check(
    rule: &dyn ReadinessRule,
    outcome: ReadinessOutcome,
    message: Option<String>,
) -> ReadinessCheck {
    ReadinessCheck {
        code: rule.code(),
        category: rule.category(),
        severity: rule.severity(),
        blocking: rule.blocking(),
        phase: rule.phase(),
        outcome,
        message,
    }
}

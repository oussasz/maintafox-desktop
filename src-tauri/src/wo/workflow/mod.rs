//! WO lifecycle workflow SSOT (Option B).
//!
//! Statuses represent lifecycle. Actions represent work. Readiness is a rule engine.

pub mod state_machine;
pub mod transition;
pub mod events;
pub mod readiness;
pub mod actions;

pub use state_machine::{
    assert_action_allowed, guard_wo_transition, WoAction, WoStatus,
};

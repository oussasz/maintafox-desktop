//! Intervention Request (DI) bounded context.
//!
//! Phase 2 - Sub-phase 04 - PRD §6.4.
//!
//! The DI module is the formal intake gate for all reactive and semi-reactive
//! maintenance demand. A DI preserves the original field signal so that demand
//! can be reviewed, screened, approved, and converted into executable work
//! without losing the origin context.
//!
//! Sub-module layout:
//!   `domain`  — enums, structs, state machine, transition guard, code generator
//!   `queries` — list, get, create, update, transition log, recurrence queries

pub mod attachments;
pub mod audit;
pub mod conversion;
pub mod domain;
pub mod permissions;
pub mod queries;
pub mod review;
pub mod sla;

#[cfg(test)]
mod conversion_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

//! Personnel bounded context (PRD §6.6).
//!
//! Workforce registry: positions, schedules, personnel master, contractor companies,
//! rate cards, and execution authorizations.

pub mod assignment_history;
pub mod availability;
pub mod domain;
pub mod import;
pub mod photos;
pub mod queries;
pub mod reports;
pub mod skills;
pub mod teams;

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod tests;

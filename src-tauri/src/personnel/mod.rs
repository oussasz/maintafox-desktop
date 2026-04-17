//! Personnel bounded context (PRD §6.6).
//!
//! Workforce registry: positions, schedules, personnel master, contractor companies,
//! rate cards, and execution authorizations.

pub mod domain;
pub mod queries;
pub mod skills;
pub mod availability;
pub mod teams;
pub mod import;
pub mod reports;

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod tests;

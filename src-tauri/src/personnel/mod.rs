//! Personnel bounded context (PRD §6.6).
//!
//! Workforce registry: positions, schedules, personnel master, contractor companies,
//! rate cards, and execution authorizations.

pub mod assignment_history;
pub mod availability;
pub mod calendar;
pub mod capacity;
pub mod domain;
pub mod import;
pub mod photos;
pub mod queries;
pub mod reference_lists;
pub mod reports;
pub mod skills;
pub mod teams;
pub mod timetable;

#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod legacy_values_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod reference_lists_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod update_clear_tests;

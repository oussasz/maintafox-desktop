//! Planning and scheduling bounded context (PRD §6.16).

pub mod break_in_reasons;
pub mod domain;
pub mod lifecycle;
pub mod load;
pub mod person_load;
pub mod qualifications;
pub mod queries;
pub mod readiness;
pub mod scheduler;
pub mod scheduling;
pub mod weekly;
pub mod weekly_schedule;
pub mod windows;
pub mod wo_sync;

#[cfg(test)]
mod tests;

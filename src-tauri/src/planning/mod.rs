//! Planning and scheduling bounded context (PRD §6.16).

pub mod domain;
pub mod load;
pub mod person_load;
pub mod qualifications;
pub mod queries;
pub mod readiness;
pub mod scheduling;
pub mod windows;
pub mod wo_sync;

#[cfg(test)]
mod tests;

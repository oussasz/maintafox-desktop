//! Planning and scheduling bounded context (PRD §6.16).

pub mod domain;
pub mod load;
pub mod person_load;
pub mod queries;
pub mod scheduling;
pub mod wo_sync;

#[cfg(test)]
mod tests;

//! Preventive Maintenance bounded context (PRD §6.9).

pub mod domain;
pub mod generation_control;
pub mod queries;
pub mod scheduler;
pub mod wo_content;

#[cfg(test)]
mod tests;

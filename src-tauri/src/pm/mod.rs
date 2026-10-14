//! Preventive Maintenance bounded context (PRD §6.9).

pub mod compliance;
pub mod domain;
pub mod generation;
pub mod generation_control;
pub mod plan_tools;
pub mod queries;
pub mod schedule;
pub mod scheduler;
pub mod wo_content;

#[cfg(test)]
mod benchmark_tests;
#[cfg(test)]
mod tests;

//! Inventory bounded context (PRD §6.8).

pub mod domain;
pub mod controls;
pub mod procurement;
pub mod queries;
pub mod valuation;

#[cfg(test)]
mod tests;

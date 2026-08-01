//! Inventory bounded context (PRD §6.8).

pub mod controls;
pub mod domain;
pub mod procurement;
pub mod queries;
pub mod suppliers;
pub mod valuation;

#[cfg(test)]
mod tests;

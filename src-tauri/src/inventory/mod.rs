//! Inventory bounded context (PRD §6.8).

pub mod approvals;
pub mod controls;
pub mod count_sync;
pub mod domain;
pub mod fx;
pub mod issue;
pub mod ledger;
pub mod lots;
pub mod negative_stock;
pub mod periods;
pub mod procurement;
pub mod queries;
pub mod reservations;
pub mod suppliers;
pub mod unit_conversions;
pub mod valuation;

#[cfg(test)]
mod benchmark_tests;
#[cfg(test)]
mod negative_stock_tests;
#[cfg(test)]
mod over_quantity_tests;
#[cfg(test)]
mod reservations_tests;
#[cfg(test)]
mod tests;

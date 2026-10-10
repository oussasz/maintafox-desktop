//! Sync envelope contracts and checkpoint persistence (PRD §8).
//!
//! Universal Entity Sync Standard: see `docs/engineering/SYNC_STRATEGY.md`.

pub mod conflicts;
pub mod domain;
pub mod foundation_stage;
pub mod identity;
pub mod materialize;
pub mod media;
pub mod ops_stage;
pub mod queries;
pub mod registry;

#[cfg(test)]
mod convergence_tests;
#[cfg(test)]
mod foundation_tests;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod tests;

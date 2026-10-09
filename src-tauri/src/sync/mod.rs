//! Sync envelope contracts and checkpoint persistence (PRD §8).
//!
//! Universal Entity Sync Standard: see `docs/engineering/SYNC_STRATEGY.md`.

pub mod domain;
pub mod foundation_stage;
pub mod materialize;
pub mod media;
pub mod ops_stage;
pub mod queries;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod convergence_tests;

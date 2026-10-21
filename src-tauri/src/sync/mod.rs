//! Sync envelope contracts and checkpoint persistence (PRD §8).
//!
//! Universal Entity Sync Standard: see `docs/engineering/SYNC_STRATEGY.md`.

pub(crate) mod apply_hooks;
pub mod capture;
pub mod code_clash;
pub mod conflicts;
pub mod device_code;
pub mod domain;
pub mod identity;
pub mod materialize;
pub mod media;
pub mod queries;
pub mod registry;
pub(crate) mod row_codec;
pub(crate) mod row_layout;
pub mod row_spec;
pub mod row_state;
pub mod seeded_identity;
pub mod transport;

#[cfg(test)]
mod convergence_tests;
#[cfg(test)]
mod foundation_tests;
#[cfg(test)]
mod harness_tests;
#[cfg(test)]
mod history_ids_tests;
#[cfg(test)]
mod hook_round_trip_tests;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod row_sync_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

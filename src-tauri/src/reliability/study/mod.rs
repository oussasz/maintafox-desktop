//! Reliability study workspace (RAMS module): fleet view, study records (step 1), life analysis
//! with the age-replacement cost model (step 2), FMECA (step 3), RCM decision (step 4), system
//! model with advanced tools (step 5), decision log (step 6) and the data-quality worklist.

pub mod advanced;
pub mod analysis;
pub mod clock;
pub mod data_quality;
pub mod decision_sync;
pub mod decisions;
pub mod domain;
pub mod fleet_view;
pub mod fmeca;
pub mod life;
pub mod pm_interval;
pub mod rcm;
pub mod records;
pub mod scales;
pub mod store;
pub mod system_model;

#[cfg(test)]
mod tests;

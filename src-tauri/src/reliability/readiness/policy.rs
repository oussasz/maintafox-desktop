//! Thresholds and policy constants for RAM data-readiness evaluation.
//! Single source of truth — keep aligned with migration `v_ram_data_quality_issues` and `compute.rs` callers.

/// Default minimum eligible failure events for statistical adequacy (`min_sample_n`).
pub const MIN_SAMPLE_N_DEFAULT: i64 = 5;

/// DQ score at or above which badge can be green (when no blocking issues).
pub const DQ_GREEN_THRESHOLD: f64 = 0.85;

/// ISO 14224-style completeness percent considered acceptable for analytics.
pub const COMPLETENESS_GREEN_THRESHOLD: f64 = 85.0;

/// Exposure log lookback for native `MISSING_EXPOSURE` blocking rule (days).
pub const EXPOSURE_LOOKBACK_DAYS: i64 = 90;

/// Inspection checkpoint coverage below which `MISSING_INSPECTION_COVERAGE` warning fires.
pub const INSPECTION_COVERAGE_WARNING_THRESHOLD: f64 = 0.85;

/// Eligible event count below which `LOW_SAMPLE` warning fires.
pub const LOW_SAMPLE_EVENT_THRESHOLD: i64 = 5;

/// JSON flag value for MTBF-eligible unplanned events (`compute::eligible_unplanned_mtbf`).
pub const ELIGIBLE_FLAGS_JSON_TRUE: &str = "{\"eligible_unplanned_mtbf\":true}";

/// JSON flag value for ineligible (planned) events.
pub const ELIGIBLE_FLAGS_JSON_FALSE: &str = "{\"eligible_unplanned_mtbf\":false}";

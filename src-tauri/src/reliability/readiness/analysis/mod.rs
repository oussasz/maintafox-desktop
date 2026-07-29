//! Scientific analysis layer for readiness PoC outputs (decomposition, sensitivity, baseline).

pub mod baseline;
pub mod decomposition;
pub mod root_cause;
pub mod sensitivity;

pub use baseline::{compute_baseline_comparison, BaselineComparison};
pub use decomposition::{
    assign_waterfall_reason, compute_decomposition, DecompositionOutput, OverlapBucket,
    WaterfallBucket, WaterfallReason,
};
pub use root_cause::{build_root_cause, RootCauseOutput};
pub use sensitivity::{
    run_mapping_sensitivity, run_nmin_sensitivity, MappingSensitivityRow, NminSensitivityRow,
};

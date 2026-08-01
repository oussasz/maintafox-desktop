use serde::{Deserialize, Serialize};

use super::policy::{DQ_GREEN_THRESHOLD, EXPOSURE_LOOKBACK_DAYS, INSPECTION_COVERAGE_WARNING_THRESHOLD};

/// Evaluation profile — no hidden defaults; CLI must select explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationProfile {
    /// CMMS documentary readiness: completeness + DQ; no exposure blocking.
    CmmsDocumentary,
    /// Full native RAM readiness including exposure and analysis gates.
    StrictRam,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadinessIssue {
    pub issue_code: String,
    pub severity: String,
}

#[derive(Debug, Clone)]
pub struct BlockingInput {
    pub exposure_hours: f64,
    pub data_quality_score: f64,
    pub eligible_event_count: i64,
    pub min_sample_n: i64,
    /// Eligible events lacking failure-mode coding (proxy or native).
    pub eligible_missing_failure_mode_count: i64,
    pub inspection_coverage_ratio: Option<f64>,
}

/// Evaluate data-quality issues for an asset under the given profile.
#[must_use]
pub fn evaluate_issues(input: &BlockingInput, profile: EvaluationProfile) -> Vec<ReadinessIssue> {
    let mut issues = Vec::new();

    if input.eligible_missing_failure_mode_count > 0 {
        issues.push(ReadinessIssue {
            issue_code: "MISSING_FAILURE_MODE".into(),
            severity: "blocking".into(),
        });
    }

    if profile == EvaluationProfile::StrictRam && input.exposure_hours <= 0.0 {
        issues.push(ReadinessIssue {
            issue_code: "MISSING_EXPOSURE".into(),
            severity: "blocking".into(),
        });
    }

    if input.data_quality_score < DQ_GREEN_THRESHOLD || input.eligible_event_count < input.min_sample_n.max(1) {
        issues.push(ReadinessIssue {
            issue_code: "LOW_SAMPLE".into(),
            severity: "warning".into(),
        });
    }

    if profile == EvaluationProfile::StrictRam {
        if let Some(cov) = input.inspection_coverage_ratio {
            if cov < INSPECTION_COVERAGE_WARNING_THRESHOLD {
                issues.push(ReadinessIssue {
                    issue_code: "MISSING_INSPECTION_COVERAGE".into(),
                    severity: "warning".into(),
                });
            }
        }
    }

    issues
}

/// Blocking issues only (severity == "blocking").
#[must_use]
pub fn blocking_codes(issues: &[ReadinessIssue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == "blocking")
        .map(|i| i.issue_code.clone())
        .collect()
}

/// Documented lookback constant for manifest / README (native DB rule).
#[must_use]
pub const fn exposure_lookback_days() -> i64 {
    EXPOSURE_LOOKBACK_DAYS
}

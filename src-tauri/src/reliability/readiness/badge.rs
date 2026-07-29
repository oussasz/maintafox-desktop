use serde::{Deserialize, Serialize};

use super::blocking::{blocking_codes, ReadinessIssue};
use super::policy::DQ_GREEN_THRESHOLD;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BadgeResult {
    pub badge: String,
    pub blocking_issue_codes: Vec<String>,
}

/// Same rules as `get_ram_equipment_quality_badge` in `queries.rs`.
#[must_use]
pub fn evaluate_badge(data_quality_score: Option<f64>, issues: &[ReadinessIssue]) -> BadgeResult {
    let blocking_issue_codes = blocking_codes(issues);
    let badge = if !blocking_issue_codes.is_empty() {
        "red"
    } else if data_quality_score.map(|s| s >= DQ_GREEN_THRESHOLD).unwrap_or(false) {
        "green"
    } else {
        "yellow"
    }
    .to_string();

    BadgeResult {
        badge,
        blocking_issue_codes,
    }
}

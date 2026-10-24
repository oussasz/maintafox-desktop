//! Four-eyes on stock count variances above the plant's tolerance (D11, CMMS Advisor decision
//! register 2026-10-22/23, migration 259).
//!
//! Tolerance. One plant-wide pair of optional settings (Settings > Inventory), read when a count is
//! entered, on every count path (count-session line, count from the stock-to-reconcile list):
//! - [`SETTING_VALUE_THRESHOLD`]: |variance| × the location's unit cost (average cost, plant
//!   currency);
//! - [`SETTING_PERCENT_THRESHOLD`]: |variance| / system quantity × 100.
//!
//! A variance is above the tolerance ("critique", `is_critical` on the line) when ANY threshold that
//! is set is exceeded — "exceeds" is strictly greater (`>`): a variance exactly at a threshold is
//! within it. A blank threshold (never set: no row) is ignored, never read as zero. Both blank:
//! strict, every variance is above. An article without a unit cost at the location is judged on the
//! percent only, and is strict when the percent is blank. A system quantity of zero or less is always
//! above (no percent can be computed). No variance is never above.
//!
//! Approval. Every variance is approved with a note of at least 8 characters, as before. Within the
//! tolerance the counter may approve it. Above it, the approver is someone other than the counter
//! (`counted_by_id`, the signed-in person who entered the counted quantity), unless the plant's site
//! opted out of four-eyes (the same per-site rule as permits and reliability decisions,
//! `reliability::study::decisions::SELF_APPROVAL_SETTING`): stores have no site yet, so an
//! organisation with exactly ONE planning site follows that site's rule and any other organisation
//! applies four-eyes ([`store_rule`]). Under the opt-out the counter's approval is flagged
//! `self_approved` ("auto-approbation"), shown on screen and in the audit. A line whose counter was
//! not recorded (entered before this version) cannot show the separation and is entered again.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};
use crate::reliability::study::decisions::SelfApprovalRule;

/// Plant setting (optional decimal, plant currency): a variance whose value is above it needs a
/// second person.
pub const SETTING_VALUE_THRESHOLD: &str = "inventory.count_four_eyes_value_threshold";
/// Plant setting (optional decimal, percent of the system quantity): a variance above it needs a
/// second person.
pub const SETTING_PERCENT_THRESHOLD: &str = "inventory.count_four_eyes_percent_threshold";

/// Refusal: the counter approves a variance above the tolerance where four-eyes applies.
pub const REFUSED_SELF_APPROVAL: &str = "GATE_INV_COUNT_SELF_APPROVAL";
/// Refusal: a variance above the tolerance whose counter was not recorded (entered before the
/// update): the separation cannot be shown, the line is entered again.
pub const REFUSED_COUNTER_UNKNOWN: &str = "GATE_INV_COUNT_COUNTER_UNKNOWN";

/// Quantities closer than this are no variance.
const QTY_TOLERANCE: f64 = 1e-9;

/// The plant's four-eyes tolerance (`None`: blank).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CountTolerance {
    pub value_threshold: Option<f64>,
    pub percent_threshold: Option<f64>,
}

impl CountTolerance {
    /// The current plant settings.
    pub async fn read<C: ConnectionTrait>(db: &C) -> AppResult<Self> {
        Ok(Self {
            value_threshold: crate::settings::catalog::optional_decimal_setting(db, SETTING_VALUE_THRESHOLD).await?,
            percent_threshold: crate::settings::catalog::optional_decimal_setting(db, SETTING_PERCENT_THRESHOLD)
                .await?,
        })
    }

    /// Whether a variance is above the tolerance (see the module documentation). `unit_cost`: the
    /// location's unit cost, `None` when the article has none there.
    pub fn is_above(&self, variance_qty: f64, system_qty: f64, unit_cost: Option<f64>) -> bool {
        let variance = variance_qty.abs();
        if variance <= QTY_TOLERANCE {
            return false;
        }
        if system_qty <= 0.0 {
            return true;
        }
        let by_value = match (self.value_threshold, unit_cost) {
            (Some(threshold), Some(cost)) => Some(variance * cost > threshold),
            _ => None,
        };
        let by_percent = self
            .percent_threshold
            .map(|threshold| variance / system_qty * 100.0 > threshold);
        match (by_value, by_percent) {
            (None, None) => true,
            (value, percent) => value.unwrap_or(false) || percent.unwrap_or(false),
        }
    }
}

/// What the tolerance said of one count, kept in its audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToleranceVerdict {
    pub tolerance: CountTolerance,
    pub unit_cost: Option<f64>,
    pub above_tolerance: bool,
}

impl ToleranceVerdict {
    pub fn audit_json(&self) -> serde_json::Value {
        serde_json::json!({
            "value_threshold": self.tolerance.value_threshold,
            "percent_threshold": self.tolerance.percent_threshold,
            "unit_cost": self.unit_cost,
            "above_tolerance": self.above_tolerance,
            "rule": "above when any threshold set is exceeded (>); both blank: every variance",
        })
    }
}

/// The tolerance applied to a count of an article at a location, in the caller's transaction.
pub async fn evaluate<C: ConnectionTrait>(
    conn: &C,
    article_id: i64,
    location_id: i64,
    variance_qty: f64,
    system_qty: f64,
) -> AppResult<ToleranceVerdict> {
    let tolerance = CountTolerance::read(conn).await?;
    let unit_cost = crate::inventory::valuation::location_average_cost(conn, article_id, location_id).await?;
    Ok(ToleranceVerdict {
        tolerance,
        unit_cost,
        above_tolerance: tolerance.is_above(variance_qty, system_qty, unit_cost),
    })
}

/// The four-eyes rule of the stores, and the site it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreRule {
    pub rule: SelfApprovalRule,
    /// The organisation's only planning site, when it has exactly one.
    pub site_id: Option<i64>,
}

/// Stores have no site yet: an organisation with exactly one planning site (live node whose type
/// can own work, outside a draft structure, as `settings::site` lists them) follows that site's
/// opt-out; any other organisation applies four-eyes.
pub async fn store_rule<C: ConnectionTrait>(conn: &C) -> AppResult<StoreRule> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT n.id FROM org_nodes n
             JOIN org_node_types t ON t.id = n.node_type_id AND t.can_own_work = 1
             WHERE n.deleted_at IS NULL
               AND NOT EXISTS (SELECT 1 FROM org_structure_models m
                               WHERE m.id = n.structure_model_id AND m.status = 'draft')
             LIMIT 2"
                .to_string(),
        ))
        .await?;
    if rows.len() != 1 {
        return Ok(StoreRule {
            rule: SelfApprovalRule::SiteUnresolved,
            site_id: None,
        });
    }
    let site: i64 = rows[0].try_get("", "id")?;
    let allowed =
        crate::settings::site::site_setting(conn, site, crate::reliability::study::decisions::SELF_APPROVAL_SETTING)
            .await?
            == Some(true);
    Ok(StoreRule {
        rule: if allowed {
            SelfApprovalRule::Allowed
        } else {
            SelfApprovalRule::FourEyes
        },
        site_id: Some(site),
    })
}

/// The four-eyes check of an approval of a counted variance by `approver_id`. Returns whether it is
/// a self-approval ("auto-approbation", allowed only under the store's opt-out); refused where
/// four-eyes applies, or when the counter of a variance above the tolerance was not recorded.
pub async fn check_approval<C: ConnectionTrait>(
    conn: &C,
    above_tolerance: bool,
    counted_by_id: Option<i64>,
    approver_id: i64,
) -> AppResult<bool> {
    if !above_tolerance {
        return Ok(false);
    }
    let Some(counter) = counted_by_id else {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_COUNTER_UNKNOWN}: this variance is above the four-eyes tolerance and its counter was not \
             recorded (entered before the update): enter the counted quantity again."
        )]));
    };
    if counter != approver_id {
        return Ok(false);
    }
    if store_rule(conn).await?.rule == SelfApprovalRule::Allowed {
        return Ok(true);
    }
    Err(AppError::ValidationFailed(vec![format!(
        "{REFUSED_SELF_APPROVAL}: you counted this variance, which is above the four-eyes tolerance: another \
         person approves it."
    )]))
}

#[cfg(test)]
mod tests {
    use super::CountTolerance;

    fn tolerance(value: Option<f64>, percent: Option<f64>) -> CountTolerance {
        CountTolerance {
            value_threshold: value,
            percent_threshold: percent,
        }
    }

    #[test]
    fn any_threshold_set_and_exceeded_makes_a_variance_above_the_tolerance() {
        // No variance is never above.
        assert!(!tolerance(None, None).is_above(0.0, 10.0, Some(2.0)));
        // Both blank: strict.
        assert!(tolerance(None, None).is_above(0.5, 10.0, Some(2.0)));
        // Value only: 3 × 2 = 6.
        assert!(tolerance(Some(5.0), None).is_above(-3.0, 10.0, Some(2.0)));
        assert!(
            !tolerance(Some(6.0), None).is_above(-3.0, 10.0, Some(2.0)),
            "exactly at it: within"
        );
        // Percent only: 3 / 10 = 30 %.
        assert!(tolerance(None, Some(29.0)).is_above(3.0, 10.0, None));
        assert!(
            !tolerance(None, Some(30.0)).is_above(3.0, 10.0, None),
            "exactly at it: within"
        );
        // Either exceeded.
        assert!(tolerance(Some(100.0), Some(20.0)).is_above(3.0, 10.0, Some(2.0)));
        assert!(tolerance(Some(5.0), Some(50.0)).is_above(3.0, 10.0, Some(2.0)));
        assert!(!tolerance(Some(10.0), Some(50.0)).is_above(3.0, 10.0, Some(2.0)));
    }

    #[test]
    fn without_unit_cost_only_the_percent_counts_and_blank_percent_is_strict() {
        assert!(!tolerance(Some(1.0), Some(50.0)).is_above(3.0, 10.0, None));
        assert!(tolerance(Some(1_000.0), None).is_above(3.0, 10.0, None));
    }

    #[test]
    fn a_system_quantity_of_zero_or_less_is_always_above() {
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(1.0, 0.0, Some(0.01)));
        assert!(tolerance(Some(1_000.0), Some(99.0)).is_above(6.0, -5.0, Some(0.01)));
    }
}

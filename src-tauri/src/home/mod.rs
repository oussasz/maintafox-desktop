//! Home page (start centre): the figures each person starts the day with, by role — in the spirit
//! of the Maximo Start Center and the SAP Fiori launchpad.
//!
//! - **My work** (`my_work`): the signed-in person's own work orders, requests, inspection rounds,
//!   PM occurrences and notifications to acknowledge.
//! - **Planning** (`planning`): the schedule backlog by readiness, this week's load against the
//!   crews' capacity and schedule compliance.
//! - **Supervision** (`supervision`): open requests against their SLA, the OT backlog by nature
//!   and urgency, overdue PM, stock to reorder, permits and lockouts, critical assets down.
//!
//! Every figure is the owning module's own definition, read through that module's function or
//! SQL predicate (`di::stats`, `di::sla`, `wo::backlog`, `analytics::as_of`, `pm::queries`,
//! `inspection::insights`, `planning`, `inventory`, `permit`, `notifications`,
//! `assets::taxonomy_reference`); nothing is recomputed differently here. A section is `None`
//! when the person may not open the module: the page then shows no panel and no count for it.
//! A failed query is an error for the whole panel (shown with a retry), never a 0.

pub mod my_work;
pub mod planning;
pub mod supervision;

#[cfg(test)]
mod tests;

use sea_orm::QueryResult;

use crate::errors::{AppError, AppResult};
use crate::reference::urgency_scale::UrgencyCode;

/// Records listed under a figure (the full list is one click away in the module).
pub const ITEM_LIMIT: u64 = 6;

/// Periods offered by the period selector (days).
pub const PERIOD_CHOICES: [i64; 2] = [7, 30];

/// A period from the selector; any other value is refused (never silently replaced).
pub fn checked_period(days: i64) -> AppResult<i64> {
    if PERIOD_CHOICES.contains(&days) {
        Ok(days)
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "GATE_HOME_PERIOD_INVALID: the period must be one of {PERIOD_CHOICES:?} days."
        )]))
    }
}

/// SQL `CASE` ranking an urgency code column on the shared scale (1 = most urgent, 99 = none).
pub(crate) fn urgency_rank_sql(column: &str) -> String {
    let arms: String = UrgencyCode::ALL
        .iter()
        .map(|c| format!(" WHEN '{}' THEN {}", c.as_str(), c.rank()))
        .collect();
    format!("CASE {column}{arms} ELSE 99 END")
}

pub(crate) fn decode<T: sea_orm::TryGetable>(row: &QueryResult, col: &str) -> AppResult<T> {
    row.try_get::<T>("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("home decode '{col}': {e}")))
}

/// The single row of an aggregate query; its absence is an error, never a 0.
pub(crate) fn one_row(row: Option<QueryResult>, what: &str) -> AppResult<QueryResult> {
    row.ok_or_else(|| AppError::Internal(anyhow::anyhow!("home {what}: aggregate returned no row")))
}

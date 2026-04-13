//! Asset cross-module binding summary service.
//!
//! Phase 2 - Sub-phase 02 - File 03 - Sprint S3.
//!
//! Returns per-domain counts for modules that are already linked to an asset,
//! and placeholder statuses for modules not yet implemented.
//!
//! In this initial phase only `linked_document_count` queries the database.
//! All other domains return `status: "not_implemented"` with `count: null`.

use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

/// Binding status for a single domain.
#[derive(Debug, Clone, Serialize)]
pub struct DomainBindingEntry {
    /// `"available"` when the count query is live, `"not_implemented"` otherwise.
    pub status: String,
    /// Actual count if available, `None` when domain is not yet implemented.
    pub count: Option<i64>,
}

/// Full binding summary for an asset.
#[derive(Debug, Clone, Serialize)]
pub struct AssetBindingSummary {
    pub asset_id: i64,
    pub linked_di_count: DomainBindingEntry,
    pub linked_wo_count: DomainBindingEntry,
    pub linked_pm_plan_count: DomainBindingEntry,
    pub linked_failure_event_count: DomainBindingEntry,
    pub linked_document_count: DomainBindingEntry,
    pub linked_iot_signal_count: DomainBindingEntry,
    pub linked_erp_mapping_count: DomainBindingEntry,
}

fn not_implemented() -> DomainBindingEntry {
    DomainBindingEntry {
        status: "not_implemented".to_string(),
        count: None,
    }
}

/// Fetch cross-module binding summary for a given asset.
///
/// Only `linked_document_count` is live-queried in this phase.
/// All other domains return placeholder entries.
pub async fn get_asset_binding_summary(
    db: &DatabaseConnection,
    asset_id: i64,
) -> AppResult<AssetBindingSummary> {
    // Live query: count active document links for this asset.
    let doc_count = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM asset_document_links \
                 WHERE asset_id = ? AND valid_to IS NULL",
                [asset_id.into()],
            ))
            .await?
            .expect("COUNT always returns a row");
        row.try_get::<i64>("", "cnt").unwrap_or(0)
    };

    Ok(AssetBindingSummary {
        asset_id,
        linked_di_count: not_implemented(),
        linked_wo_count: not_implemented(),
        linked_pm_plan_count: not_implemented(),
        linked_failure_event_count: not_implemented(),
        linked_document_count: DomainBindingEntry {
            status: "available".to_string(),
            count: Some(doc_count),
        },
        linked_iot_signal_count: not_implemented(),
        linked_erp_mapping_count: not_implemented(),
    })
}

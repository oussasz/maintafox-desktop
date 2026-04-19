//! Asset cross-module binding summary service.
//!
//! Returns per-domain counts for modules linked to an equipment (`asset_id` is the `equipment.id`).

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

fn available_count(n: i64) -> DomainBindingEntry {
    DomainBindingEntry {
        status: "available".to_string(),
        count: Some(n),
    }
}

/// Fetch cross-module binding summary for a given asset.
pub async fn get_asset_binding_summary(
    db: &DatabaseConnection,
    asset_id: i64,
) -> AppResult<AssetBindingSummary> {
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

    // One round-trip: DI / WO / PM / failure events + integration fields on `equipment`.
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                (SELECT COUNT(*) FROM intervention_requests WHERE asset_id = ?) AS di_cnt,
                (SELECT COUNT(*) FROM work_orders WHERE equipment_id = ?) AS wo_cnt,
                (SELECT COUNT(*) FROM pm_plans
                 WHERE LOWER(asset_scope_type) = 'equipment' AND asset_scope_id = ?) AS pm_cnt,
                (SELECT COUNT(*) FROM failure_events WHERE equipment_id = ?) AS fe_cnt,
                COALESCE(
                  (SELECT CASE WHEN iot_asset_id IS NOT NULL AND TRIM(iot_asset_id) != '' THEN 1 ELSE 0 END
                   FROM equipment WHERE id = ?),
                  0
                ) AS iot_cnt,
                COALESCE(
                  (SELECT
                      (CASE WHEN erp_asset_id IS NOT NULL AND TRIM(erp_asset_id) != '' THEN 1 ELSE 0 END) +
                      (CASE WHEN erp_functional_location IS NOT NULL AND TRIM(erp_functional_location) != '' THEN 1 ELSE 0 END)
                   FROM equipment WHERE id = ?),
                  0
                ) AS erp_cnt",
            [
                asset_id.into(),
                asset_id.into(),
                asset_id.into(),
                asset_id.into(),
                asset_id.into(),
                asset_id.into(),
            ],
        ))
        .await?
        .expect("binding summary subqueries return a row");

    let di_cnt = row.try_get::<i64>("", "di_cnt").unwrap_or(0);
    let wo_cnt = row.try_get::<i64>("", "wo_cnt").unwrap_or(0);
    let pm_cnt = row.try_get::<i64>("", "pm_cnt").unwrap_or(0);
    let fe_cnt = row.try_get::<i64>("", "fe_cnt").unwrap_or(0);
    let iot_cnt = row.try_get::<i64>("", "iot_cnt").unwrap_or(0);
    let erp_cnt = row.try_get::<i64>("", "erp_cnt").unwrap_or(0);

    Ok(AssetBindingSummary {
        asset_id,
        linked_di_count: available_count(di_cnt),
        linked_wo_count: available_count(wo_cnt),
        linked_pm_plan_count: available_count(pm_cnt),
        linked_failure_event_count: available_count(fe_cnt),
        linked_document_count: available_count(doc_count),
        linked_iot_signal_count: available_count(iot_cnt),
        linked_erp_mapping_count: available_count(erp_cnt),
    })
}

//! Single sync entity registry (Phase 2.2).
//!
//! One table describes every business table that takes part in replication, and a second one
//! lists the tables that deliberately stay on this device. Everything sync-related (apply order,
//! which inbound types can be applied, conflict handling, per-user delivery, the server allowlist
//! in `shared/sync/entity-registry.json`) is read from here instead of scattered constants.
//!
//! The CI test `registry_covers_every_table` fails when a migration adds a table that is in
//! neither list, so a new business table cannot silently stay unsynchronised.
//!
//! Coverage today (see `sync_status` in the JSON export):
//! - `synced`: staged on write and applied inbound;
//! - `staged_only`: staged on write, inbound rows wait as `deferred` until step 2.4 adds a
//!   materializer;
//! - `not_staged`: registered for step 2.4, nothing is staged yet.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// Who receives a replicated row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncScope {
    /// Every device of the tenant.
    Tenant,
    /// Only the devices of the owning user (`owner_user_sync_id`).
    User,
}

/// How concurrent changes to the same row are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Mutable row: a concurrent edit becomes a `sync_conflicts` entry that a person resolves.
    OperatorReview,
    /// Insert-only history row: rows are never overwritten, a replayed row is a no-op.
    AppendOnly,
}

/// Why a table never leaves this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOnlyCategory {
    /// Sync machinery itself (outbox, inbox, checkpoints, conflicts) and the migration ledger.
    SyncControl,
    /// Login sessions, trusted devices and device-level configuration.
    SessionOrDevice,
    /// Local backup schedule and run history.
    Backup,
    /// Licence, entitlement and activation state, owned by the control plane.
    Licensing,
    /// Secret references and connection profiles; secrets never travel.
    Secrets,
    /// Import staging rows that only live until the import is committed.
    ImportStaging,
    /// Job queues, report runs and generated artifacts of this device.
    JobQueue,
    /// Projection rebuilt from replicated rows.
    DerivedProjection,
    /// Catalog defined by the application version and seeded by migrations.
    AppCatalog,
    /// Legacy leftover kept only for old foreign keys.
    Legacy,
    /// Files and archived snapshots kept on the device that made them.
    DeviceArchive,
}

#[derive(Debug, Clone, Copy)]
pub struct SyncEntityDef {
    /// Wire name of the entity (`entity_type` in envelopes).
    pub entity_type: &'static str,
    /// Local SQLite table holding the rows.
    pub table: &'static str,
    pub scope: SyncScope,
    /// For user-scoped rows: local column holding the owning `user_accounts.id`.
    pub owner_user_column: Option<&'static str>,
    /// A stager writes this entity to the outbox inside the business transaction.
    pub has_stager: bool,
    /// Inbound rows of this entity are applied to the local table.
    pub has_materializer: bool,
    pub conflict_policy: ConflictPolicy,
    /// Entity types that must be applied before this one.
    pub parents: &'static [&'static str],
    /// Known gap or decision left to a later step.
    pub note: Option<&'static str>,
}

impl SyncEntityDef {
    pub fn sync_status(&self) -> &'static str {
        match (self.has_stager, self.has_materializer) {
            (true, true) => "synced",
            (true, false) => "staged_only",
            (false, true) => "inbound_only",
            (false, false) => "not_staged",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LocalOnlyTable {
    pub table: &'static str,
    pub category: LocalOnlyCategory,
}

pub const SYNC_ENTITIES: &[SyncEntityDef] = &[
    SyncEntityDef { entity_type: "activity_events", table: "activity_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["user_accounts", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "admin_change_events", table: "admin_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["roles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "analytics_contract_versions", table: "analytics_contract_versions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "app_settings", table: "app_settings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: Some("Mixes device and tenant settings (setting_scope); step 2.4 must sync only tenant-scoped keys.") },
    SyncEntityDef { entity_type: "article_cost_profiles", table: "article_cost_profiles", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["external_companies", "lookup_values", "articles"], note: None },
    SyncEntityDef { entity_type: "article_families", table: "article_families", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "articles", table: "articles", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_locations", "warehouses", "lookup_values", "reference_values", "article_families"], note: None },
    SyncEntityDef { entity_type: "asset_document_links", table: "asset_document_links", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "asset_external_ids", table: "asset_external_ids", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment"], note: None },
    SyncEntityDef { entity_type: "asset_meter_readings", table: "asset_meter_readings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["equipment_meters", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "asset_photos", table: "asset_photos", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "equipment"], note: None },
    SyncEntityDef { entity_type: "audit_events", table: "audit_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["user_accounts"], note: Some("Device audit trail; step 2.4 decides between append-only sync and a dedicated audit channel.") },
    SyncEntityDef { entity_type: "budget_actuals", table: "budget_actuals", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "personnel", "user_accounts", "equipment", "work_orders", "cost_centers", "budget_versions"], note: None },
    SyncEntityDef { entity_type: "budget_alert_configs", table: "budget_alert_configs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["roles", "user_accounts", "cost_centers", "budget_versions"], note: None },
    SyncEntityDef { entity_type: "budget_alert_events", table: "budget_alert_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "notifications", "notification_events", "cost_centers", "budget_versions", "budget_alert_configs"], note: None },
    SyncEntityDef { entity_type: "budget_commitments", table: "budget_commitments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders", "cost_centers", "budget_versions", "purchase_orders"], note: None },
    SyncEntityDef { entity_type: "budget_erp_cost_center_master", table: "budget_erp_cost_center_master", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["cost_centers"], note: None },
    SyncEntityDef { entity_type: "budget_forecast_runs", table: "budget_forecast_runs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "budget_versions"], note: None },
    SyncEntityDef { entity_type: "budget_forecasts", table: "budget_forecasts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["cost_centers", "budget_versions", "budget_forecast_runs"], note: None },
    SyncEntityDef { entity_type: "budget_lines", table: "budget_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "cost_centers", "budget_versions"], note: None },
    SyncEntityDef { entity_type: "budget_variance_reviews", table: "budget_variance_reviews", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "cost_centers", "budget_versions"], note: None },
    SyncEntityDef { entity_type: "budget_versions", table: "budget_versions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "capacity_rules", table: "capacity_rules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "certification_types", table: "certification_types", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "closeout_validation_policies", table: "closeout_validation_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes"], note: None },
    SyncEntityDef { entity_type: "currency_exchange_rates", table: "currency_exchange_rates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "cost_centers", table: "cost_centers", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "data_integrity_findings", table: "data_integrity_findings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_order_failure_details", "work_order_downtime_segments", "work_orders"], note: None },
    SyncEntityDef { entity_type: "data_integrity_repair_actions", table: "data_integrity_repair_actions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["user_accounts", "data_integrity_findings"], note: None },
    SyncEntityDef { entity_type: "delay_reason_codes", table: "delay_reason_codes", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "delegated_admin_policies", table: "delegated_admin_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["roles", "org_nodes", "teams"], note: None },
    SyncEntityDef { entity_type: "di_attachments", table: "di_attachments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["intervention_requests", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "di_change_events", table: "di_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "di_review_events", table: "di_review_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["intervention_requests"], note: None },
    SyncEntityDef { entity_type: "di_sla_rules", table: "di_sla_rules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "di_state_transition_log", table: "di_state_transition_log", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "document_acknowledgements", table: "document_acknowledgements", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel"], note: None },
    SyncEntityDef { entity_type: "equipment", table: "equipment", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "org_nodes", "equipment_classes", "lookup_values"], note: None },
    SyncEntityDef { entity_type: "equipment_classes", table: "equipment_classes", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "equipment_criticality_assessments", table: "equipment_criticality_assessments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "equipment_hierarchy", table: "equipment_hierarchy", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment"], note: None },
    SyncEntityDef { entity_type: "equipment_lifecycle_events", table: "equipment_lifecycle_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "equipment_meters", table: "equipment_meters", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "reference_values"], note: None },
    SyncEntityDef { entity_type: "equipment_spare_parts", table: "equipment_spare_parts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "articles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "event_tree_models", table: "event_tree_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "external_companies", table: "external_companies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "external_company_contacts", table: "external_company_contacts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["external_companies"], note: None },
    SyncEntityDef { entity_type: "failure_codes", table: "failure_codes", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["failure_hierarchies"], note: None },
    SyncEntityDef { entity_type: "failure_events", table: "failure_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["failure_codes", "reference_values", "equipment", "work_orders", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "failure_hierarchies", table: "failure_hierarchies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "fmeca_analyses", table: "fmeca_analyses", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "fmeca_item_parts", table: "fmeca_item_parts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["articles", "fmeca_items"], note: None },
    SyncEntityDef { entity_type: "fmeca_items", table: "fmeca_items", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["fmeca_analyses", "pm_plan_versions", "pm_plans", "ram_ishikawa_diagrams", "reference_values", "work_orders"], note: None },
    SyncEntityDef { entity_type: "fta_models", table: "fta_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "goods_receipt_lines", table: "goods_receipt_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_locations", "articles", "purchase_order_lines", "goods_receipts", "reference_values"], note: None },
    SyncEntityDef { entity_type: "goods_receipts", table: "goods_receipts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["purchase_orders", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inspection_anomalies", table: "inspection_anomalies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inspection_results", "inspection_rounds"], note: None },
    SyncEntityDef { entity_type: "inspection_checkpoints", table: "inspection_checkpoints", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "inspection_template_versions", "reference_values"], note: None },
    SyncEntityDef { entity_type: "inspection_evidence", table: "inspection_evidence", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["inspection_results"], note: None },
    SyncEntityDef { entity_type: "inspection_results", table: "inspection_results", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "inspection_checkpoints", "inspection_rounds"], note: None },
    SyncEntityDef { entity_type: "inspection_rounds", table: "inspection_rounds", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "inspection_template_versions", "inspection_templates", "user_accounts", "inspection_schedules", "teams"], note: None },
    SyncEntityDef { entity_type: "inspection_schedules", table: "inspection_schedules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inspection_templates", "equipment_meters", "personnel", "teams"], note: None },
    SyncEntityDef { entity_type: "inspection_template_versions", table: "inspection_template_versions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inspection_templates"], note: None },
    SyncEntityDef { entity_type: "inspection_templates", table: "inspection_templates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes"], note: None },
    SyncEntityDef { entity_type: "integration_exceptions", table: "integration_exceptions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["posted_export_batches", "budget_actuals", "budget_forecasts"], note: None },
    SyncEntityDef { entity_type: "intervention_requests", table: "intervention_requests", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inspection_anomalies", "equipment", "org_nodes", "user_accounts", "reference_values", "di_sla_rules"], note: None },
    SyncEntityDef { entity_type: "inventory_article_equivalents", table: "inventory_article_equivalents", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["articles"], note: None },
    SyncEntityDef { entity_type: "inventory_count_lines", table: "inventory_count_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inventory_transactions", "stock_locations", "warehouses", "articles", "inventory_count_sessions", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_count_sessions", table: "inventory_count_sessions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_locations", "warehouses", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_document_links", table: "inventory_document_links", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inventory_suppliers", "purchase_orders", "repairable_orders", "articles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_lots", table: "inventory_lots", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["articles", "goods_receipt_lines", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_mutation_audit_links", table: "inventory_mutation_audit_links", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["inventory_transactions", "inventory_count_lines", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_reconciliation_findings", table: "inventory_reconciliation_findings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_locations", "warehouses", "articles", "inventory_reconciliation_runs"], note: None },
    SyncEntityDef { entity_type: "inventory_reconciliation_runs", table: "inventory_reconciliation_runs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_state_events", table: "inventory_state_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "inventory_supplier_article_sources", table: "inventory_supplier_article_sources", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["articles", "inventory_suppliers"], note: None },
    SyncEntityDef { entity_type: "inventory_supplier_contacts", table: "inventory_supplier_contacts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inventory_suppliers"], note: None },
    SyncEntityDef { entity_type: "inventory_supplier_prices", table: "inventory_supplier_prices", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["articles", "inventory_suppliers", "lookup_values", "reference_values"], note: None },
    SyncEntityDef { entity_type: "inventory_suppliers", table: "inventory_suppliers", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["external_companies", "lookup_values", "personnel"], note: None },
    SyncEntityDef { entity_type: "inventory_transactions", table: "inventory_transactions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["inventory_lots", "lookup_values", "reference_values", "stock_reservations", "stock_locations", "warehouses", "articles", "work_orders", "goods_receipts", "inventory_count_sessions", "repairable_orders", "user_accounts"], note: Some("Materializer inserts movements but does not update stock_balances (derived); step 2.4 must rebuild balances on apply.") },
    SyncEntityDef { entity_type: "inventory_valuation_policies", table: "inventory_valuation_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["lookup_values", "articles", "article_families", "warehouses"], note: None },
    SyncEntityDef { entity_type: "inventory_valuation_periods", table: "inventory_valuation_periods", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "inventory_valuation_snapshot_lines", table: "inventory_valuation_snapshot_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["inventory_valuation_periods", "articles", "warehouses"], note: None },
    SyncEntityDef { entity_type: "library_documents", table: "library_documents", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "lookup_domains", table: "lookup_domains", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "lookup_value_aliases", table: "lookup_value_aliases", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["lookup_values"], note: None },
    SyncEntityDef { entity_type: "lookup_values", table: "lookup_values", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["lookup_domains"], note: None },
    SyncEntityDef { entity_type: "loto_card_print_jobs", table: "loto_card_print_jobs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["work_permits", "permit_isolations", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "markov_models", table: "markov_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "mc_models", table: "mc_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "media_attachments", table: "media_attachments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["equipment", "intervention_requests", "work_orders"], note: None },
    SyncEntityDef { entity_type: "notification_acknowledgements", table: "notification_acknowledgements", scope: SyncScope::User, owner_user_column: Some("acknowledged_by_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "notifications"], note: None },
    SyncEntityDef { entity_type: "notification_categories", table: "notification_categories", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "notification_escalation_policies", table: "notification_escalation_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "notification_escalation_steps", table: "notification_escalation_steps", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["notification_escalation_policies", "teams", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "notification_events", table: "notification_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "notification_preferences", table: "notification_preferences", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["notification_categories", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "notification_rules", table: "notification_rules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["notification_categories", "notification_escalation_policies"], note: None },
    SyncEntityDef { entity_type: "notifications", table: "notifications", scope: SyncScope::User, owner_user_column: Some("recipient_user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["roles", "user_accounts", "notification_events"], note: Some("Role-targeted rows (recipient_user_id NULL) have no owner; step 2.4 must fan them out per user or keep them tenant-wide.") },
    SyncEntityDef { entity_type: "org_change_events", table: "org_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "org_entity_bindings", table: "org_entity_bindings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes"], note: None },
    SyncEntityDef { entity_type: "org_node_responsibilities", table: "org_node_responsibilities", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "personnel", "teams"], note: None },
    SyncEntityDef { entity_type: "org_node_types", table: "org_node_types", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_structure_models"], note: None },
    SyncEntityDef { entity_type: "org_nodes", table: "org_nodes", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_structure_models", "org_node_types"], note: None },
    SyncEntityDef { entity_type: "org_structure_models", table: "org_structure_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "org_type_relationship_rules", table: "org_type_relationship_rules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_structure_models", "org_node_types"], note: None },
    SyncEntityDef { entity_type: "permit_handover_logs", table: "permit_handover_logs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["work_permits", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "permit_isolations", table: "permit_isolations", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_permits", "user_accounts", "reference_values"], note: None },
    SyncEntityDef { entity_type: "permit_suspensions", table: "permit_suspensions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_permits", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "permit_types", table: "permit_types", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "personnel", table: "personnel", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["external_companies", "reference_values", "teams", "org_nodes", "positions"], note: None },
    SyncEntityDef { entity_type: "personnel_assignment_history", table: "personnel_assignment_history", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "personnel", "positions", "teams", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "personnel_authorizations", table: "personnel_authorizations", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "certification_types"], note: None },
    SyncEntityDef { entity_type: "personnel_availability_blocks", table: "personnel_availability_blocks", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "personnel"], note: None },
    SyncEntityDef { entity_type: "personnel_certifications", table: "personnel_certifications", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["certification_types", "personnel"], note: None },
    SyncEntityDef { entity_type: "personnel_rate_cards", table: "personnel_rate_cards", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "cost_centers"], note: None },
    SyncEntityDef { entity_type: "personnel_skills", table: "personnel_skills", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "reference_values"], note: None },
    SyncEntityDef { entity_type: "personnel_team_assignments", table: "personnel_team_assignments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "personnel"], note: None },
    SyncEntityDef { entity_type: "planning_windows", table: "planning_windows", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "equipment"], note: None },
    SyncEntityDef { entity_type: "pm_executions", table: "pm_executions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_orders", "pm_occurrences"], note: None },
    SyncEntityDef { entity_type: "pm_findings", table: "pm_findings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders", "intervention_requests", "pm_executions"], note: None },
    SyncEntityDef { entity_type: "pm_occurrence_transitions", table: "pm_occurrence_transitions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["user_accounts", "pm_occurrences"], note: None },
    SyncEntityDef { entity_type: "pm_occurrences", table: "pm_occurrences", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders", "pm_plan_versions", "pm_plans", "equipment"], note: None },
    SyncEntityDef { entity_type: "pm_plan_versions", table: "pm_plan_versions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["pm_plans", "equipment_meters"], note: None },
    SyncEntityDef { entity_type: "pm_plans", table: "pm_plans", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["lookup_values", "teams", "equipment", "reference_values", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "pm_trigger_events", table: "pm_trigger_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["pm_occurrences", "pm_plan_versions", "pm_plans"], note: None },
    SyncEntityDef { entity_type: "policy_snapshots", table: "policy_snapshots", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "position_change_events", table: "position_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["positions"], note: None },
    SyncEntityDef { entity_type: "positions", table: "positions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["qualification_requirement_profiles"], note: None },
    SyncEntityDef { entity_type: "posted_export_batches", table: "posted_export_batches", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "procurement_approval_limits", table: "procurement_approval_limits", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["roles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "procurement_requisition_lines", table: "procurement_requisition_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["lookup_values", "stock_reservations", "stock_locations", "articles", "procurement_requisitions"], note: None },
    SyncEntityDef { entity_type: "procurement_requisitions", table: "procurement_requisitions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "purchase_order_lines", table: "purchase_order_lines", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_reservations", "articles", "procurement_requisition_lines", "purchase_orders"], note: None },
    SyncEntityDef { entity_type: "purchase_orders", table: "purchase_orders", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inventory_suppliers", "external_companies", "procurement_requisitions", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "qualification_profile_skills", table: "qualification_profile_skills", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "qualification_requirement_profiles"], note: None },
    SyncEntityDef { entity_type: "qualification_requirement_profiles", table: "qualification_requirement_profiles", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["certification_types"], note: None },
    SyncEntityDef { entity_type: "ram_advanced_guardrails", table: "ram_advanced_guardrails", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "ram_expert_sign_offs", table: "ram_expert_sign_offs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "ram_ishikawa_diagrams", table: "ram_ishikawa_diagrams", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment"], note: None },
    SyncEntityDef { entity_type: "rbac_settings", table: "rbac_settings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "rbd_models", table: "rbd_models", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "rcm_decisions", table: "rcm_decisions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["pm_plan_versions", "pm_plans", "rcm_studies", "reference_values", "work_orders", "fmeca_items"], note: None },
    SyncEntityDef { entity_type: "rcm_studies", table: "rcm_studies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "reference_aliases", table: "reference_aliases", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values"], note: None },
    SyncEntityDef { entity_type: "reference_domains", table: "reference_domains", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "reference_sets", table: "reference_sets", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_domains", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "reference_validation_reports", table: "reference_validation_reports", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_sets", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "reference_value_migrations", table: "reference_value_migrations", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "reference_values", table: "reference_values", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_sets"], note: Some("Seeded catalog rows got per-replica ids in migration 148; step 2.4 must align seeded ids across replicas (identity::seeded_entity_sync_id).") },
    SyncEntityDef { entity_type: "reliability_beta_benchmarks", table: "reliability_beta_benchmarks", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "reliability_cost_profiles", table: "reliability_cost_profiles", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment_classes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "reliability_dashboard_policies", table: "reliability_dashboard_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "reliability_studies", table: "reliability_studies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "pm_plans", "user_accounts", "fmeca_analyses", "rcm_studies", "rbd_models", "fta_models"], note: None },
    SyncEntityDef { entity_type: "reliability_study_assets", table: "reliability_study_assets", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reliability_studies", "equipment"], note: None },
    SyncEntityDef { entity_type: "reliability_study_decisions", table: "reliability_study_decisions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reliability_studies", "equipment", "equipment_meters", "pm_plans", "pm_plan_versions", "work_orders", "user_accounts", "fmeca_items", "rcm_decisions", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "reliability_study_record_events", table: "reliability_study_record_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["reliability_studies", "failure_events", "work_orders", "equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "repairable_orders", table: "repairable_orders", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["inventory_suppliers", "stock_reservations", "purchase_order_lines", "stock_locations", "articles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "report_schedules", table: "report_schedules", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["report_templates", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "report_templates", table: "report_templates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "retention_policies", table: "retention_policies", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "role_permissions", table: "role_permissions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["roles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "role_templates", table: "role_templates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "roles", table: "roles", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: Some("System roles are seeded on every replica; seeded rows must use identity::seeded_entity_sync_id.") },
    SyncEntityDef { entity_type: "runtime_exposure_logs", table: "runtime_exposure_logs", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment"], note: None },
    SyncEntityDef { entity_type: "saved_activity_filters", table: "saved_activity_filters", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "schedule_break_ins", table: "schedule_break_ins", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "schedule_commitments", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "schedule_candidates", table: "schedule_candidates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "personnel", "reference_values", "intervention_requests", "work_orders", "pm_occurrences"], note: None },
    SyncEntityDef { entity_type: "schedule_change_log", table: "schedule_change_log", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["schedule_commitments", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "schedule_commitments", table: "schedule_commitments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "teams", "schedule_candidates", "work_orders", "pm_occurrences", "intervention_requests", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "weekly_schedules", table: "weekly_schedules", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "weekly_schedule_events", table: "weekly_schedule_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["weekly_schedules", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "weekly_schedule_baseline_items", table: "weekly_schedule_baseline_items", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["weekly_schedules", "schedule_commitments", "teams", "personnel"], note: None },
    SyncEntityDef { entity_type: "schedule_details", table: "schedule_details", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values"], note: None },
    SyncEntityDef { entity_type: "settings_change_events", table: "settings_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "site_setting_events", table: "site_setting_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["org_nodes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "site_settings", table: "site_settings", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "user_accounts"], note: Some("Per-site rules (settings::site); no row = the rule's strict default.") },
    SyncEntityDef { entity_type: "skill_categories", table: "skill_categories", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "skill_definitions", table: "skill_definitions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["skill_categories"], note: None },
    SyncEntityDef { entity_type: "stock_locations", table: "stock_locations", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["warehouses"], note: None },
    SyncEntityDef { entity_type: "stock_reservations", table: "stock_reservations", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["stock_locations", "warehouses", "articles", "work_orders", "user_accounts"], note: Some("Work order link is resolved by sync id; the row is deferred until that work order is present.") },
    SyncEntityDef { entity_type: "team_skill_requirements", table: "team_skill_requirements", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "skill_definitions"], note: None },
    SyncEntityDef { entity_type: "teams", table: "teams", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes"], note: None },
    SyncEntityDef { entity_type: "training_attendance", table: "training_attendance", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "training_sessions"], note: None },
    SyncEntityDef { entity_type: "training_sessions", table: "training_sessions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["certification_types", "personnel"], note: None },
    SyncEntityDef { entity_type: "unit_conversions", table: "unit_conversions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "articles", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "user_accounts", table: "user_accounts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "user_analytics_dashboard_layouts", table: "user_analytics_dashboard_layouts", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "user_dashboard_layouts", table: "user_dashboard_layouts", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "user_dismissals", table: "user_dismissals", scope: SyncScope::User, owner_user_column: Some("user_id"), has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "user_scope_assignments", table: "user_scope_assignments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "roles", "org_nodes", "teams"], note: None },
    SyncEntityDef { entity_type: "warehouses", table: "warehouses", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "weibull_fit_results", table: "weibull_fit_results", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["equipment", "failure_codes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "wo_action_events", table: "wo_action_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["work_orders"], note: None },
    SyncEntityDef { entity_type: "wo_change_events", table: "wo_change_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "wo_state_transition_log", table: "wo_state_transition_log", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &[], note: None },
    SyncEntityDef { entity_type: "work_order_attachments", table: "work_order_attachments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_delay_segments", table: "work_order_delay_segments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "reference_values", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_downtime_segments", table: "work_order_downtime_segments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_execution_events", table: "work_order_execution_events", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["user_accounts", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_failure_details", table: "work_order_failure_details", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["failure_codes", "reference_values", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_interveners", table: "work_order_interveners", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_orders", "reference_values", "personnel"], note: None },
    SyncEntityDef { entity_type: "work_order_parts", table: "work_order_parts", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "lookup_values", "stock_reservations", "work_orders", "articles", "stock_locations"], note: None },
    SyncEntityDef { entity_type: "work_order_statuses", table: "work_order_statuses", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &[], note: None },
    SyncEntityDef { entity_type: "work_order_tasks", table: "work_order_tasks", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_tools", table: "work_order_tools", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders"], note: None },
    SyncEntityDef { entity_type: "work_order_verifications", table: "work_order_verifications", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "work_orders"], note: None },
    SyncEntityDef { entity_type: "work_orders", table: "work_orders", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "ram_ishikawa_diagrams", "closeout_validation_policies", "inspection_anomalies", "equipment", "teams", "intervention_requests", "reference_values", "work_order_statuses", "org_nodes", "schedule_class_versions"], note: None },
    SyncEntityDef { entity_type: "work_permits", table: "work_permits", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders", "permit_types", "equipment", "org_nodes"], note: None },
    SyncEntityDef { entity_type: "wo_cost_estimates", table: "wo_cost_estimates", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "cost_centers", "work_orders"], note: None },
    SyncEntityDef { entity_type: "wo_cost_settlements", table: "wo_cost_settlements", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts", "budget_versions", "cost_centers", "work_orders"], note: None },
    SyncEntityDef { entity_type: "budget_periods", table: "budget_periods", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["user_accounts"], note: None },
    SyncEntityDef { entity_type: "qualification_overrides", table: "qualification_overrides", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::AppendOnly, parents: &["personnel", "work_orders", "work_permits", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "qualification_requirements", table: "qualification_requirements", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["work_orders", "permit_types", "certification_types", "reference_values", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "work_calendar_holidays", table: "work_calendar_holidays", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "site_work_weeks", table: "site_work_weeks", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["org_nodes", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "schedule_class_versions", table: "schedule_class_versions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["reference_values", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "team_schedule_assignments", table: "team_schedule_assignments", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["teams", "reference_values", "user_accounts"], note: None },
    SyncEntityDef { entity_type: "personnel_schedule_exceptions", table: "personnel_schedule_exceptions", scope: SyncScope::Tenant, owner_user_column: None, has_stager: true, has_materializer: true, conflict_policy: ConflictPolicy::OperatorReview, parents: &["personnel", "reference_values", "user_accounts"], note: None },
];

pub const LOCAL_ONLY_TABLES: &[LocalOnlyTable] = &[
    LocalOnlyTable {
        table: "sync_checkpoint",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_conflicts",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_inbox",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_outbox",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_rejections",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_repair_actions",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_replay_runs",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_replica_identity",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_entity_state",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_transport_state",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_media_uploads",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_media_downloads",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_history_foreign_rows",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_capture_journal",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_capture_suppression",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_capture_baseline",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "sync_pending_refs",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "seaql_migrations",
        category: LocalOnlyCategory::SyncControl,
    },
    LocalOnlyTable {
        table: "app_sessions",
        category: LocalOnlyCategory::SessionOrDevice,
    },
    LocalOnlyTable {
        table: "trusted_devices",
        category: LocalOnlyCategory::SessionOrDevice,
    },
    LocalOnlyTable {
        table: "system_config",
        category: LocalOnlyCategory::SessionOrDevice,
    },
    LocalOnlyTable {
        table: "backup_policies",
        category: LocalOnlyCategory::Backup,
    },
    LocalOnlyTable {
        table: "backup_runs",
        category: LocalOnlyCategory::Backup,
    },
    LocalOnlyTable {
        table: "entitlement_cache_state",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "entitlement_envelopes",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "license_admin_actions",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "license_api_exchange_guard",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "license_enforcement_state",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "license_event_traces",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "licensing_trust_keys",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "machine_activation_contracts",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "machine_activation_lineage",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "machine_activation_state",
        category: LocalOnlyCategory::Licensing,
    },
    // Vendor administrator recovery codes spent on this PC and the notices they raise for this PC's
    // administrators (`auth::recovery_code`): local, like the first-administrator marker.
    LocalOnlyTable {
        table: "admin_recovery_codes_used",
        category: LocalOnlyCategory::Licensing,
    },
    LocalOnlyTable {
        table: "admin_recovery_notices",
        category: LocalOnlyCategory::SessionOrDevice,
    },
    LocalOnlyTable {
        table: "connection_profiles",
        category: LocalOnlyCategory::Secrets,
    },
    LocalOnlyTable {
        table: "secure_secret_refs",
        category: LocalOnlyCategory::Secrets,
    },
    LocalOnlyTable {
        table: "asset_import_batches",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "asset_import_events",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "asset_import_staging",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "personnel_import_batches",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "personnel_import_rows",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "reference_import_batches",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "reference_import_rows",
        category: LocalOnlyCategory::ImportStaging,
    },
    LocalOnlyTable {
        table: "computation_jobs",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "report_runs",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "report_artifacts",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "event_export_runs",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "pm_generation_runs",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "pm_generation_control",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "inspection_offline_queue",
        category: LocalOnlyCategory::JobQueue,
    },
    LocalOnlyTable {
        table: "notification_deliveries",
        category: LocalOnlyCategory::JobQueue,
    },
    // When each notification watcher started on this device (`notifications::watchers`).
    LocalOnlyTable {
        table: "notification_watch_state",
        category: LocalOnlyCategory::SessionOrDevice,
    },
    LocalOnlyTable {
        table: "stock_balances",
        category: LocalOnlyCategory::DerivedProjection,
    },
    // Computed on each device from synchronised personnel and certification data
    // (`qualification::readiness`, `qualification::expiry_alerts`).
    LocalOnlyTable {
        table: "personnel_readiness_snapshots",
        category: LocalOnlyCategory::DerivedProjection,
    },
    LocalOnlyTable {
        table: "training_expiry_alert_events",
        category: LocalOnlyCategory::DerivedProjection,
    },
    // Archived records (snapshot of the source row with its local ids). Only tests write them yet;
    // an archive stays on the device that made it until archiving is synchronised as a whole.
    LocalOnlyTable {
        table: "archive_items",
        category: LocalOnlyCategory::DeviceArchive,
    },
    LocalOnlyTable {
        table: "archive_payloads",
        category: LocalOnlyCategory::DeviceArchive,
    },
    LocalOnlyTable {
        table: "archive_actions",
        category: LocalOnlyCategory::DeviceArchive,
    },
    // Links between event rows by local id; nothing writes them outside tests.
    LocalOnlyTable {
        table: "event_links",
        category: LocalOnlyCategory::DerivedProjection,
    },
    // Computed per equipment and period on the device that runs the analysis or closes the work
    // order (`reliability::queries`); each device computes its own.
    LocalOnlyTable {
        table: "reliability_kpi_snapshots",
        category: LocalOnlyCategory::DerivedProjection,
    },
    // Recomputed per period on the device that runs the signal computation (`inspection::signals`).
    LocalOnlyTable {
        table: "inspection_reliability_signals",
        category: LocalOnlyCategory::DerivedProjection,
    },
    // Recomputed on each device by the candidate refresh (`planning::queries`).
    LocalOnlyTable {
        table: "scheduling_conflicts",
        category: LocalOnlyCategory::DerivedProjection,
    },
    LocalOnlyTable {
        table: "permissions",
        category: LocalOnlyCategory::AppCatalog,
    },
    LocalOnlyTable {
        table: "permission_dependencies",
        category: LocalOnlyCategory::AppCatalog,
    },
    LocalOnlyTable {
        table: "users",
        category: LocalOnlyCategory::Legacy,
    },
];

/// Look up a registered entity type.
pub fn entity(entity_type: &str) -> Option<&'static SyncEntityDef> {
    SYNC_ENTITIES.iter().find(|e| e.entity_type == entity_type)
}

/// Look up a registered entity type, failing with a sync error when it is unknown.
pub fn require_entity(entity_type: &str) -> AppResult<&'static SyncEntityDef> {
    entity(entity_type).ok_or_else(|| AppError::SyncError(format!("Unknown sync entity type '{entity_type}'.")))
}

/// True when inbound rows of this type can be applied now.
pub fn is_materializable(entity_type: &str) -> bool {
    entity(entity_type).is_some_and(|e| e.has_materializer)
}

/// Entity types whose inbound rows can be applied now.
pub fn materializable_entity_types() -> Vec<&'static str> {
    SYNC_ENTITIES
        .iter()
        .filter(|e| e.has_materializer)
        .map(|e| e.entity_type)
        .collect()
}

fn priorities() -> &'static HashMap<&'static str, u16> {
    static PRIORITIES: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    PRIORITIES.get_or_init(|| {
        // Depth in the parent graph: an entity is applied after all of its parents. Back edges
        // are ignored here; the test `registry_parent_graph_is_acyclic` keeps them out of the table.
        fn depth(
            entity_type: &'static str,
            memo: &mut HashMap<&'static str, u16>,
            visiting: &mut Vec<&'static str>,
        ) -> u16 {
            if let Some(d) = memo.get(entity_type) {
                return *d;
            }
            if visiting.contains(&entity_type) {
                return 0;
            }
            visiting.push(entity_type);
            let parents = entity(entity_type).map(|e| e.parents).unwrap_or(&[]);
            let mut level = 0u16;
            for parent in parents {
                level = level.max(depth(parent, memo, visiting) + 1);
            }
            visiting.pop();
            memo.insert(entity_type, level);
            level
        }
        let mut memo = HashMap::new();
        for e in SYNC_ENTITIES {
            depth(e.entity_type, &mut memo, &mut Vec::new());
        }
        memo
    })
}

/// Apply order (lower first). Unregistered types sort last.
pub fn apply_priority(entity_type: &str) -> u16 {
    priorities().get(entity_type).copied().unwrap_or(u16::MAX)
}

#[derive(Debug, Serialize)]
struct ExportedEntity {
    entity_type: &'static str,
    scope: SyncScope,
    conflict_policy: ConflictPolicy,
    apply_priority: u16,
    parents: Vec<&'static str>,
    sync_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct ExportedRegistry {
    #[serde(rename = "$comment")]
    comment: &'static str,
    registry_version: u32,
    entities: Vec<ExportedEntity>,
    local_only_tables: BTreeMap<&'static str, LocalOnlyCategory>,
}

/// Bump when the exported shape changes (not when entries are added).
const REGISTRY_EXPORT_VERSION: u32 = 1;

/// Pretty JSON written to `shared/sync/entity-registry.json` for the control-plane allowlist.
pub fn registry_export_json() -> AppResult<String> {
    let entities = SYNC_ENTITIES
        .iter()
        .map(|e| ExportedEntity {
            entity_type: e.entity_type,
            scope: e.scope,
            conflict_policy: e.conflict_policy,
            apply_priority: apply_priority(e.entity_type),
            parents: e.parents.to_vec(),
            sync_status: e.sync_status(),
            note: e.note,
        })
        .collect();
    let export = ExportedRegistry {
        comment: "Generated from src-tauri/src/sync/registry.rs by the test `registry_export_matches_shared_file`. Do not edit by hand.",
        registry_version: REGISTRY_EXPORT_VERSION,
        entities,
        local_only_tables: LOCAL_ONLY_TABLES.iter().map(|t| (t.table, t.category)).collect(),
    };
    let mut json = serde_json::to_string_pretty(&export)?;
    json.push('\n');
    Ok(json)
}

//! Migration 162 — Sync identity for every replicated table and the capture journal (Phase 2.4).
//!
//! - `sync_capture_journal`: written by the capture triggers that `sync::capture` installs on
//!   every replicated table after the migrations. One row per business write, in the same
//!   transaction as the write; the sync engine turns it into an outbox envelope.
//! - `sync_capture_suppression`: one-row flag set while inbound rows are applied, so applying a
//!   row received from the server is not captured and sent back.
//! - `sync_pending_refs`: links that point back along a dependency cycle (for example a plan's
//!   current version) and whose target has not been received yet; completed when it arrives.
//! - `sync_capture_baseline`: entity types whose existing rows were queued once for sync.
//! - `entity_sync_id` + unique index on every replicated table keyed by `id` that had none.
//! - Rows that every device seeds itself (migration seeds, startup seeders, tenant bootstrap) are
//!   re-keyed to `sync::identity::seeded_entity_sync_id(entity_type, natural key)`, so the same
//!   catalog row has the same identity on every database (`sync::seeded_identity`, shared with
//!   the startup seeders so the key formats cannot drift).
//! - Unsent outbox changes staged before this version carry the old hand-written payload format
//!   that the generic materializer cannot apply; they are superseded and the rows are queued again
//!   by the baseline capture in the new format.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261011_000162_sync_capture_identity"
    }
}

/// Replicated tables keyed by `id` that had no `entity_sync_id` when this migration was written.
const TABLES_WITHOUT_SYNC_ID: &[&str] = &[
    "activity_events",
    "admin_change_events",
    "app_settings",
    "archive_actions",
    "archive_items",
    "archive_payloads",
    "article_families",
    "asset_external_ids",
    "asset_meter_readings",
    "audit_events",
    "budget_actuals",
    "budget_commitments",
    "budget_erp_cost_center_master",
    "budget_forecast_runs",
    "budget_forecasts",
    "budget_variance_reviews",
    "capacity_rules",
    "cost_centers",
    "delay_reason_codes",
    "delegated_admin_policies",
    "di_change_events",
    "di_review_events",
    "di_sla_rules",
    "di_state_transition_log",
    "equipment_hierarchy",
    "event_links",
    "external_companies",
    "external_company_contacts",
    "fmeca_item_parts",
    "goods_receipt_lines",
    "goods_receipts",
    "inventory_article_equivalents",
    "inventory_count_lines",
    "inventory_count_sessions",
    "inventory_document_links",
    "inventory_mutation_audit_links",
    "inventory_reconciliation_findings",
    "inventory_reconciliation_runs",
    "inventory_state_events",
    "inventory_supplier_article_sources",
    "inventory_supplier_contacts",
    "inventory_supplier_prices",
    "inventory_suppliers",
    "inventory_valuation_policies",
    "library_documents",
    "lookup_value_aliases",
    "notification_acknowledgements",
    "notification_categories",
    "notification_escalation_policies",
    "notification_escalation_steps",
    "notification_events",
    "notification_preferences",
    "notification_rules",
    "notifications",
    "org_change_events",
    "org_entity_bindings",
    "org_node_responsibilities",
    "org_type_relationship_rules",
    "personnel_assignment_history",
    "personnel_authorizations",
    "personnel_availability_blocks",
    "personnel_rate_cards",
    "personnel_skills",
    "personnel_team_assignments",
    "planning_windows",
    "pm_executions",
    "pm_findings",
    "pm_occurrence_transitions",
    "pm_trigger_events",
    "policy_snapshots",
    "position_change_events",
    "positions",
    "procurement_requisition_lines",
    "procurement_requisitions",
    "purchase_order_lines",
    "purchase_orders",
    "qualification_profile_skills",
    "ram_advanced_guardrails",
    "reference_aliases",
    "reference_domains",
    "reference_validation_reports",
    "reference_value_migrations",
    "reliability_beta_benchmarks",
    "reliability_dashboard_policies",
    "repairable_orders",
    "report_schedules",
    "report_templates",
    "retention_policies",
    "role_templates",
    "saved_activity_filters",
    "schedule_break_ins",
    "schedule_candidates",
    "schedule_change_log",
    "schedule_commitments",
    "schedule_details",
    "scheduling_conflicts",
    "settings_change_events",
    "team_skill_requirements",
    "urgency_levels",
    "wo_action_events",
    "wo_change_events",
    "wo_state_transition_log",
    "work_order_delay_segments",
    "work_order_downtime_segments",
    "work_order_execution_events",
    "work_order_failure_details",
    "work_order_parts",
    "work_order_statuses",
    "work_order_tools",
    "work_order_types",
    "work_order_verifications",
];

/// Tables that already had `entity_sync_id` but no unique index on it.
const TABLES_NEEDING_UNIQUE_SYNC_ID: &[&str] = &[
    "analytics_contract_versions",
    "certification_types",
    "data_integrity_findings",
    "data_integrity_repair_actions",
    "document_acknowledgements",
    "equipment",
    "equipment_classes",
    "equipment_lifecycle_events",
    "equipment_meters",
    "event_tree_models",
    "failure_codes",
    "failure_events",
    "failure_hierarchies",
    "fmeca_analyses",
    "fmeca_items",
    "fta_models",
    "inspection_anomalies",
    "inspection_checkpoints",
    "inspection_evidence",
    "inspection_reliability_signals",
    "inspection_results",
    "inspection_rounds",
    "inspection_template_versions",
    "inspection_templates",
    "integration_exceptions",
    "lookup_domains",
    "lookup_values",
    "loto_card_print_jobs",
    "markov_models",
    "mc_models",
    "media_attachments",
    "org_node_types",
    "org_nodes",
    "org_structure_models",
    "permit_handover_logs",
    "permit_isolations",
    "permit_suspensions",
    "permit_types",
    "personnel_certifications",
    "personnel_readiness_snapshots",
    "posted_export_batches",
    "qualification_requirement_profiles",
    "ram_expert_sign_offs",
    "ram_ishikawa_diagrams",
    "rbd_models",
    "rcm_decisions",
    "rcm_studies",
    "reliability_kpi_snapshots",
    "roles",
    "runtime_exposure_logs",
    "skill_categories",
    "skill_definitions",
    "teams",
    "training_attendance",
    "training_expiry_alert_events",
    "training_sessions",
    "user_accounts",
    "user_dismissals",
    "user_scope_assignments",
    "weibull_fit_results",
    "work_permits",
    "asset_document_links",
    "asset_photos",
    "di_attachments",
    "work_order_attachments",
    "closeout_validation_policies",
];

async fn column_names(db: &dyn ConnectionTrait, table: &str) -> Result<Vec<String>, DbErr> {
    let cols = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info('{table}')"),
        ))
        .await?;
    cols.iter().map(|c| c.try_get::<String>("", "name")).collect()
}

async fn table_exists(db: &dyn ConnectionTrait, table: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

/// Adds the column when missing, gives every row without identity a random one, and makes the
/// identity unique.
async fn ensure_sync_identity(db: &dyn ConnectionTrait, table: &str) -> Result<(), DbErr> {
    if !table_exists(db, table).await? {
        return Ok(());
    }
    if !column_names(db, table).await?.iter().any(|c| c == "entity_sync_id") {
        db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN entity_sync_id TEXT NULL"))
            .await?;
    }
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT id FROM {table} WHERE entity_sync_id IS NULL OR trim(entity_sync_id) = ''"),
        ))
        .await?;
    for row in rows {
        let id: i64 = row.try_get("", "id")?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!("UPDATE {table} SET entity_sync_id = ? WHERE id = ?"),
            [uuid::Uuid::new_v4().to_string().into(), id.into()],
        ))
        .await?;
    }
    db.execute_unprepared(&format!(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_{table}_entity_sync_id ON {table}(entity_sync_id)"
    ))
    .await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_capture_journal (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type        TEXT NOT NULL,
                entity_sync_id     TEXT NULL,
                natural_key_json   TEXT NULL,
                operation          TEXT NOT NULL CHECK (operation IN ('upsert', 'delete')),
                owner_user_sync_id TEXT NULL,
                captured_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                attempt_count      INTEGER NOT NULL DEFAULT 0,
                last_error         TEXT NULL,
                CHECK (entity_sync_id IS NOT NULL OR natural_key_json IS NOT NULL)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_sync_capture_journal_entity
             ON sync_capture_journal(entity_type, entity_sync_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_capture_suppression (
                id INTEGER PRIMARY KEY CHECK (id = 1)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_pending_refs (
                id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type        TEXT NOT NULL,
                entity_sync_id     TEXT NOT NULL,
                column_name        TEXT NOT NULL,
                target_entity_type TEXT NOT NULL,
                target_sync_id     TEXT NOT NULL,
                created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE (entity_type, entity_sync_id, column_name)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS sync_capture_baseline (
                entity_type TEXT PRIMARY KEY,
                captured_at TEXT NOT NULL,
                row_count   INTEGER NOT NULL
            )",
        )
        .await?;

        for table in TABLES_WITHOUT_SYNC_ID.iter().chain(TABLES_NEEDING_UNIQUE_SYNC_ID) {
            ensure_sync_identity(db, table).await?;
        }
        crate::sync::seeded_identity::assign_seeded_identities(db, crate::sync::seeded_identity::SeededScope::AllRows)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;

        // Old hand-written payloads cannot be applied by the generic materializer: replace them.
        db.execute_unprepared(
            "UPDATE sync_outbox
             SET status = 'superseded', last_error = 'replaced by the Phase 2.4 payload format',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE status IN ('pending', 'in_flight')",
        )
        .await?;
        if table_exists(db, "sync_transport_state").await? {
            db.execute_unprepared(
                "UPDATE sync_transport_state
                 SET pending_idempotency_key = NULL, pending_request_body = NULL,
                     pending_outbox_ids_json = NULL, pending_created_at = NULL
                 WHERE id = 1",
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

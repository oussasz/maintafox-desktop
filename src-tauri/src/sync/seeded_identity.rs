//! Identity of rows that every device seeds itself (Phase 2.4).
//!
//! Migrations, the startup seeders and the tenant activation bootstrap create the same catalog
//! rows on every database (statuses, types, reference catalogs, settings, the bootstrap
//! administrator, the tenant root organisation). Such a row must have the same `entity_sync_id`
//! everywhere, otherwise two devices hold two copies of "the same" row and the first sync fails on
//! its unique code. Its identity is `seeded_entity_sync_id(entity_type, natural key)` (a UUID of
//! version 8, which also tells the sync engine never to queue it as a new local row).
//!
//! The natural-key formats below are the contract: migration 162 re-keyed existing rows with
//! them, and startup gives the same identity to rows the seeders create without one. Seeders that
//! write the identity themselves use the same formats (`seeded_entity_sync_id`).

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::identity::seeded_entity_sync_id;

/// Natural key of a reference value (`domain:code`), as used by the reference catalog seeders.
const REFERENCE_VALUE_KEY: &str = "(SELECT d.code FROM reference_sets s JOIN reference_domains d ON d.id = s.domain_id
      WHERE s.id = t.set_id) || ':' || t.code";

/// `(entity_type = table, natural key SQL over alias t, row filter)`, parents before children.
pub const SEEDED_ROWS: &[(&str, &str, &str)] = &[
    ("roles", "t.name", "1 = 1"),
    ("reference_domains", "t.code", "1 = 1"),
    (
        "reference_sets",
        "(SELECT d.code FROM reference_domains d WHERE d.id = t.domain_id) || ':' || t.version_no",
        "1 = 1",
    ),
    ("reference_values", REFERENCE_VALUE_KEY, "1 = 1"),
    ("lookup_domains", "t.domain_key", "1 = 1"),
    (
        "lookup_values",
        "(SELECT d.domain_key FROM lookup_domains d WHERE d.id = t.domain_id) || ':' || t.code",
        "1 = 1",
    ),
    ("permit_types", "t.code", "1 = 1"),
    ("warehouses", "t.code", "1 = 1"),
    (
        "stock_locations",
        "(SELECT w.code FROM warehouses w WHERE w.id = t.warehouse_id) || ':' || t.code",
        "1 = 1",
    ),
    ("failure_hierarchies", "t.name || ':' || t.version_no", "1 = 1"),
    (
        "closeout_validation_policies",
        "t.policy_name || ':' || COALESCE((SELECT o.code FROM org_nodes o WHERE o.id = t.entity_id), '')",
        "1 = 1",
    ),
    ("analytics_contract_versions", "t.contract_id || ':' || t.version_semver", "1 = 1"),
    ("app_settings", "t.setting_key || ':' || t.setting_scope", "1 = 1"),
    ("delay_reason_codes", "t.code", "1 = 1"),
    ("di_sla_rules", "t.name", "1 = 1"),
    ("notification_categories", "t.code", "1 = 1"),
    ("notification_escalation_policies", "t.name", "1 = 1"),
    (
        "notification_escalation_steps",
        "(SELECT p.name FROM notification_escalation_policies p WHERE p.id = t.escalation_policy_id) || ':' || t.level_no",
        "1 = 1",
    ),
    ("notification_rules", "t.category_code", "1 = 1"),
    ("positions", "t.code", "1 = 1"),
    ("ram_advanced_guardrails", "'default'", "1 = 1"),
    ("reliability_dashboard_policies", "t.scope_code", "1 = 1"),
    ("report_templates", "t.code", "1 = 1"),
    ("retention_policies", "t.module_code || ':' || t.archive_class", "1 = 1"),
    ("role_templates", "t.name", "1 = 1"),
    (
        "schedule_details",
        "(SELECT d.code || ':' || v.code FROM reference_values v
           JOIN reference_sets s ON s.id = v.set_id
           JOIN reference_domains d ON d.id = s.domain_id
          WHERE v.id = t.reference_value_id) || ':' || t.day_of_week",
        "1 = 1",
    ),
    ("urgency_levels", "CAST(t.level AS TEXT)", "1 = 1"),
    ("work_order_statuses", "t.code", "1 = 1"),
    ("work_order_types", "t.code", "1 = 1"),
    ("inventory_valuation_policies", "t.code", "1 = 1"),
    // Derived from the published EQUIPMENT.CLASS catalog on every device.
    ("equipment_classes", "t.code", "1 = 1"),
    // Default session / backup policies (`settings::ensure_default_policies`).
    ("policy_snapshots", "t.policy_domain || ':' || t.version_no", "1 = 1"),
    // Bootstrap administrator created by the startup seeder on every device.
    ("user_accounts", "t.username", "t.username = 'admin'"),
    (
        "user_scope_assignments",
        "'admin:Administrator:tenant'",
        "t.scope_type = 'tenant' AND t.scope_reference IS NULL
         AND t.user_id = (SELECT id FROM user_accounts WHERE username = 'admin')
         AND t.role_id = (SELECT id FROM roles WHERE name = 'Administrator')",
    ),
    // Tenant activation bootstrap: active org model, its root node type and the root node.
    (
        "org_structure_models",
        "'bootstrap-model'",
        "t.description = 'Tenant activation bootstrap'",
    ),
    (
        "org_node_types",
        "'bootstrap-root-type'",
        "t.code = 'ROOT' AND t.is_root_type = 1
         AND t.structure_model_id = (SELECT id FROM org_structure_models WHERE description = 'Tenant activation bootstrap')",
    ),
    ("org_nodes", "t.code", "t.parent_id IS NULL AND t.code LIKE 'TENANT-ROOT-%'"),
];

/// Which rows get a seeded identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeededScope {
    /// Every row matching the rules (migration 162: re-key rows created before).
    AllRows,
    /// Only rows without identity (startup: rows the seeders just created).
    MissingIdentity,
}

async fn table_exists(conn: &dyn ConnectionTrait, table: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

/// Give seeded rows their deterministic identity. A natural key that is empty, or that an earlier
/// row already holds, is left alone (a locally created duplicate keeps a random identity).
pub async fn assign_seeded_identities(conn: &dyn ConnectionTrait, scope: SeededScope) -> AppResult<()> {
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("seeded identity: {e}"));
    for (entity_type, key_sql, filter) in SEEDED_ROWS {
        if !table_exists(conn, entity_type).await? {
            continue;
        }
        let rows = conn
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT t.id AS id, {key_sql} AS natural_key,
                            (t.entity_sync_id IS NULL OR trim(t.entity_sync_id) = '') AS missing
                     FROM {entity_type} t WHERE {filter} ORDER BY t.id"
                ),
            ))
            .await?;
        let held: HashSet<String> = conn
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("SELECT entity_sync_id AS v FROM {entity_type} WHERE entity_sync_id IS NOT NULL"),
            ))
            .await?
            .iter()
            .filter_map(|r| r.try_get::<String>("", "v").ok())
            .collect();
        let mut taken = HashSet::new();
        let mut assignments = Vec::new();
        for row in rows {
            let id: i64 = row.try_get("", "id").map_err(read)?;
            let key: Option<String> = row.try_get("", "natural_key").map_err(read)?;
            let missing: bool = row.try_get("", "missing").map_err(read)?;
            let Some(key) = key.filter(|k| !k.trim().is_empty()) else {
                continue;
            };
            if !taken.insert(key.clone()) {
                continue;
            }
            if scope == SeededScope::MissingIdentity && !missing {
                continue;
            }
            let sync_id = seeded_entity_sync_id(entity_type, &key);
            if scope == SeededScope::MissingIdentity && held.contains(&sync_id) {
                // Another row (an older copy) already carries this identity.
                continue;
            }
            assignments.push((id, sync_id));
        }
        // Two passes, so a new identity never collides with an old one still held by another row.
        for (id, _) in &assignments {
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("UPDATE {entity_type} SET entity_sync_id = 'rekey-' || id WHERE id = ?"),
                [(*id).into()],
            ))
            .await?;
        }
        for (id, sync_id) in assignments {
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("UPDATE {entity_type} SET entity_sync_id = ? WHERE id = ?"),
                [sync_id.into(), id.into()],
            ))
            .await?;
        }
    }
    Ok(())
}

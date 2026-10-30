//! What the generator checks before it writes anything (read only).
//!
//! - [`ensure_demo_company_licence`]: the licence activated on this PC is the demo company's
//!   (the organisation name stored with the activation is [`super::catalog::COMPANY_NAME`]), so
//!   the generator never fills the database of a real customer.
//! - [`ensure_no_received_sync_data`]: nothing was ever received from the server or another PC.
//! - [`ensure_no_business_data`]: every table is empty except the ones a freshly activated
//!   database fills on its own ([`SYSTEM_TABLES`]) and a few with an exact rule ([`RULED_TABLES`]).
//!   Inverted on purpose: a table added by a later migration counts as company data until it is
//!   listed here.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::catalog::COMPANY_NAME;
use super::{scalar_i64, tenant_bootstrap_root};
use crate::errors::{AppError, AppResult};
use crate::sync::identity::seeded_entity_sync_id;

/// Tables whose rows a database holds as soon as the app has started once and the licence was
/// activated, whatever the company: never company data. Derived from a database built like the
/// first start (`sync::test_support::prepare_started`: migrations, system seed, catalogs, capture)
/// followed by the activation (licence record, trust keys, `db::tenant_bootstrap`, entitlement
/// cache), plus what the first administrator's creation and sign-in write (account trail, sessions,
/// audit, change capture).
pub const SYSTEM_TABLES: &[&str] = &[
    // Schema and migrations.
    "seaql_migrations",
    "sqlite_sequence",
    "analytics_contract_versions",
    // Settings (device, plant, RBAC).
    "app_settings",
    "settings_change_events",
    "system_config",
    "rbac_settings",
    "site_work_weeks",
    // Licence, activation, entitlements.
    "entitlement_cache_state",
    "entitlement_envelopes",
    "license_enforcement_state",
    "license_admin_actions",
    "license_api_exchange_guard",
    "license_event_traces",
    "licensing_trust_keys",
    "machine_activation_state",
    "machine_activation_contracts",
    "machine_activation_lineage",
    "policy_snapshots",
    "trusted_devices",
    // The first administrator: sessions, recovery codes, personal screen settings, trail.
    "app_sessions",
    "admin_recovery_acknowledgements",
    "admin_recovery_codes_used",
    "admin_recovery_notices",
    "admin_change_events",
    "user_dismissals",
    "user_dashboard_layouts",
    "user_analytics_dashboard_layouts",
    "audit_events",
    "audit_chain_stubs",
    "activity_events",
    // Sync metadata of this PC (what was received is checked by `ensure_no_received_sync_data`).
    "sync_capture_baseline",
    "sync_capture_journal",
    "sync_capture_suppression",
    "sync_checkpoint",
    "sync_entity_state",
    "sync_outbox",
    "sync_replica_identity",
    "sync_transport_state",
    // Backups of this PC.
    "backup_policies",
    "backup_runs",
    // System catalogs seeded by every installation.
    "certification_types",
    "closeout_validation_policies",
    "delay_reason_codes",
    "di_sla_rules",
    "equipment_classes",
    "failure_hierarchies",
    "inventory_valuation_policies",
    "lookup_domains",
    "lookup_values",
    "notification_categories",
    "notification_escalation_policies",
    "notification_escalation_steps",
    "notification_rules",
    "permission_dependencies",
    "permissions",
    "permit_types",
    "pm_generation_control",
    "positions",
    "ram_advanced_guardrails",
    "reliability_dashboard_policies",
    "report_templates",
    "retention_policies",
    "role_permissions",
    "role_templates",
    "schedule_class_versions",
    "schedule_details",
    "work_order_statuses",
];

/// Tables a fresh database fills with a known content; any other row is company data (checked by
/// [`ruled_offenders`]).
pub const RULED_TABLES: &[&str] = &[
    // At most one account besides the built-in "admin": the first administrator.
    "user_accounts",
    // One role assignment per allowed account.
    "user_scope_assignments",
    // Only the seeded roles (identity derived from the role name): no custom role.
    "roles",
    // Only the seeded lists and values (identity derived from `domain:code`), first versions only.
    "reference_domains",
    "reference_sets",
    "reference_values",
    // Only the organisation written by the activation (`db::tenant_bootstrap`).
    "org_structure_models",
    "org_node_types",
    "org_nodes",
    // Only the empty 'MAIN' store the migrations create, and its locations.
    "warehouses",
    "stock_locations",
];

/// Refuses unless the database holds no company data at all; the message lists what was found.
pub async fn ensure_no_business_data(db: &DatabaseConnection) -> AppResult<()> {
    let tables = text_column(
        db,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         ORDER BY name",
    )
    .await?;
    let mut found = Vec::new();
    for table in &tables {
        if SYSTEM_TABLES.contains(&table.as_str()) || RULED_TABLES.contains(&table.as_str()) {
            continue;
        }
        let n = scalar_i64(db, &format!("SELECT COUNT(*) FROM \"{table}\""), vec![])
            .await?
            .unwrap_or(0);
        if n > 0 {
            found.push(format!("{table}: {n}"));
        }
    }
    found.extend(ruled_offenders(db).await?);
    if found.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "La base contient déjà des données de l'entreprise ({}). Le générateur ne remplit qu'une base vide.",
            found.join(", ")
        )]))
    }
}

/// Rows of [`RULED_TABLES`] that a fresh database does not hold.
async fn ruled_offenders(db: &DatabaseConnection) -> AppResult<Vec<String>> {
    let mut found = Vec::new();
    let mut push = |table: &str, what: &str, n: i64| {
        if n > 0 {
            found.push(format!("{table} ({what}): {n}"));
        }
    };

    let builtin = crate::auth::builtin_admin::builtin_admin_entity_sync_id();
    let accounts = count(
        db,
        "SELECT COUNT(*) FROM user_accounts WHERE entity_sync_id IS NOT ?",
        vec![builtin.into()],
    )
    .await?;
    push(
        "user_accounts",
        "comptes en plus du premier administrateur",
        (accounts - 1).max(0),
    );
    push(
        "user_scope_assignments",
        "affectations de rôle en plus d'une par compte",
        count(
            db,
            "SELECT COUNT(*) FROM user_scope_assignments \
             WHERE id NOT IN (SELECT MIN(id) FROM user_scope_assignments GROUP BY user_id)",
            vec![],
        )
        .await?,
    );

    let roles = pairs(db, "SELECT entity_sync_id, name FROM roles").await?;
    push(
        "roles",
        "rôles créés dans l'entreprise",
        not_seeded(&roles, "roles", |name| name.to_string()),
    );

    let domains = pairs(db, "SELECT entity_sync_id, code FROM reference_domains").await?;
    push(
        "reference_domains",
        "listes créées dans l'entreprise",
        not_seeded(&domains, "reference_domains", |code| code.to_string()),
    );
    push(
        "reference_sets",
        "versions modifiées de listes",
        count(db, "SELECT COUNT(*) FROM reference_sets WHERE version_no > 1", vec![]).await?,
    );
    let values = pairs(
        db,
        "SELECT rv.entity_sync_id, rd.code || ':' || rv.code FROM reference_values rv \
         JOIN reference_sets rs ON rs.id = rv.set_id JOIN reference_domains rd ON rd.id = rs.domain_id",
    )
    .await?;
    push(
        "reference_values",
        "valeurs ajoutées dans l'entreprise",
        not_seeded(&values, "reference_values", |key| key.to_string()) + {
            // A value whose set cannot be read is not a seeded one either.
            count(
                db,
                "SELECT COUNT(*) FROM reference_values rv WHERE NOT EXISTS ( \
                 SELECT 1 FROM reference_sets rs JOIN reference_domains rd ON rd.id = rs.domain_id \
                 WHERE rs.id = rv.set_id)",
                vec![],
            )
            .await?
        },
    );

    push(
        "org_structure_models",
        "modèles d'organisation en plus de celui de l'activation",
        count(
            db,
            "SELECT COUNT(*) FROM org_structure_models WHERE entity_sync_id IS NOT ?",
            vec![seeded_entity_sync_id("org_structure_models", "bootstrap-model").into()],
        )
        .await?,
    );
    push(
        "org_node_types",
        "types de nœud en plus de la racine de l'activation",
        count(
            db,
            "SELECT COUNT(*) FROM org_node_types WHERE entity_sync_id IS NOT ?",
            vec![seeded_entity_sync_id("org_node_types", "bootstrap-root-type").into()],
        )
        .await?,
    );
    let root = tenant_bootstrap_root(db).await?.unwrap_or(-1);
    push(
        "org_nodes",
        "nœuds d'organisation en plus de la racine de l'activation",
        count(db, "SELECT COUNT(*) FROM org_nodes WHERE id <> ?", vec![root.into()]).await?,
    );

    push(
        "warehouses",
        "magasins",
        count(db, "SELECT COUNT(*) FROM warehouses WHERE code <> 'MAIN'", vec![]).await?,
    );
    push(
        "stock_locations",
        "emplacements de stock",
        count(
            db,
            "SELECT COUNT(*) FROM stock_locations WHERE warehouse_id NOT IN (SELECT id FROM warehouses WHERE code = 'MAIN')",
            vec![],
        )
        .await?,
    );
    Ok(found)
}

/// Refuses unless the licence activated on this PC is the demo company's: the organisation name
/// stored with the activation (the claim's `tenant_display_name`, the customer name of the vendor
/// console) must be [`COMPANY_NAME`] (trimmed, case-insensitive).
pub async fn ensure_demo_company_licence(db: &DatabaseConnection) -> AppResult<()> {
    let claim = crate::commands::product_license::get_activation_claim_record(db).await?;
    let Some(claim) = claim else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Aucune licence activée sur ce PC. Activez d'abord la licence du client « {COMPANY_NAME} » \
             (nom exact du client dans la console fournisseur)."
        )]));
    };
    let name = claim.tenant_display_name.unwrap_or_default();
    if !same_company(&name, COMPANY_NAME) {
        return Err(AppError::ValidationFailed(vec![format!(
            "La licence activée sur ce PC est celle de « {} », pas celle de l'entreprise de démonstration \
             « {COMPANY_NAME} ». Le générateur ne remplit jamais la base d'un client.",
            name.trim()
        )]));
    }
    Ok(())
}

/// Trimmed, case-insensitive equality of two company names.
pub(crate) fn same_company(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Refuses a database that has received anything by synchronisation (rows in the inbox, conflicts,
/// refusals, rows of another history) or applied a pull (a stored checkpoint).
pub async fn ensure_no_received_sync_data(db: &DatabaseConnection) -> AppResult<()> {
    let mut found = Vec::new();
    for (table, label) in [
        ("sync_inbox", "éléments reçus"),
        ("sync_conflicts", "conflits"),
        ("sync_rejections", "refus"),
        ("sync_history_foreign_rows", "lignes d'un autre historique"),
    ] {
        let n = count(db, &format!("SELECT COUNT(*) FROM {table}"), vec![]).await?;
        if n > 0 {
            found.push(format!("{n} {label}"));
        }
    }
    let pulled = count(
        db,
        "SELECT COUNT(*) FROM sync_checkpoint WHERE checkpoint_token IS NOT NULL OR last_sync_at IS NOT NULL",
        vec![],
    )
    .await?;
    if pulled > 0 {
        found.push("une réception déjà appliquée (point de reprise enregistré)".into());
    }
    if found.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Cette base a déjà reçu des données par synchronisation ({}). Le générateur ne remplit qu'une base \
             qui n'a jamais synchronisé.",
            found.join(", ")
        )]))
    }
}

async fn count(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> AppResult<i64> {
    Ok(scalar_i64(db, sql, values).await?.unwrap_or(0))
}

async fn text_column(db: &impl ConnectionTrait, sql: &str) -> AppResult<Vec<String>> {
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| r.try_get_by_index::<String>(0).map_err(AppError::from))
        .collect()
}

/// `(entity_sync_id, natural key)` rows.
async fn pairs(db: &impl ConnectionTrait, sql: &str) -> AppResult<Vec<(Option<String>, String)>> {
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get_by_index::<Option<String>>(0)?,
                r.try_get_by_index::<String>(1)?,
            ))
        })
        .collect()
}

/// Rows whose identity is not the seeded identity of their natural key.
fn not_seeded(rows: &[(Option<String>, String)], entity: &str, key: impl Fn(&str) -> String) -> i64 {
    rows.iter()
        .filter(|(id, natural)| id.as_deref() != Some(seeded_entity_sync_id(entity, &key(natural)).as_str()))
        .count() as i64
}

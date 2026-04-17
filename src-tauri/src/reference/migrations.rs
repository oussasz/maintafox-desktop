//! Reference value merge and usage-migration service.
//!
//! Phase 2 - Sub-phase 03 - File 02 - Sprint S3.
//!
//! Implements governed merge/migrate operations for reference values:
//!   - verifies source/target belong to the same reference domain
//!   - enforces active target value
//!   - records trace rows in `reference_value_migrations`
//!   - remaps known downstream usage probes where possible
//!   - deactivates the source value after successful operation when policy allows

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

use super::domains;
use super::protected;
use super::sets;
use super::values;

// ─── Types ────────────────────────────────────────────────────────────────────

/// A persisted row from `reference_value_migrations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceValueMigration {
    pub id: i64,
    pub domain_id: i64,
    pub from_value_id: i64,
    pub to_value_id: i64,
    pub reason_code: Option<String>,
    pub migrated_by_id: Option<i64>,
    pub migrated_at: String,
}

/// Result contract for merge/migrate operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceUsageMigrationResult {
    pub migration: ReferenceValueMigration,
    pub source_value: values::ReferenceValue,
    pub target_value: values::ReferenceValue,
    pub remapped_references: i64,
    pub source_deactivated: bool,
}

#[derive(Debug, Clone)]
struct BoundValue {
    value: values::ReferenceValue,
    domain_id: i64,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "reference_value_migrations row decode failed for column '{column}': {e}"
    ))
}

fn map_migration(row: &QueryResult) -> AppResult<ReferenceValueMigration> {
    Ok(ReferenceValueMigration {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        domain_id: row
            .try_get::<i64>("", "domain_id")
            .map_err(|e| decode_err("domain_id", e))?,
        from_value_id: row
            .try_get::<i64>("", "from_value_id")
            .map_err(|e| decode_err("from_value_id", e))?,
        to_value_id: row
            .try_get::<i64>("", "to_value_id")
            .map_err(|e| decode_err("to_value_id", e))?,
        reason_code: row
            .try_get::<Option<String>>("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        migrated_by_id: row
            .try_get::<Option<i64>>("", "migrated_by_id")
            .map_err(|e| decode_err("migrated_by_id", e))?,
        migrated_at: row
            .try_get::<String>("", "migrated_at")
            .map_err(|e| decode_err("migrated_at", e))?,
    })
}

async fn load_bound_value(
    db: &DatabaseConnection,
    value_id: i64,
) -> AppResult<BoundValue> {
    let value = values::get_value(db, value_id).await?;
    let set = sets::get_reference_set(db, value.set_id).await?;
    Ok(BoundValue {
        value,
        domain_id: set.domain_id,
    })
}

async fn validate_migration_pair(
    db: &DatabaseConnection,
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
) -> AppResult<(domains::ReferenceDomain, values::ReferenceValue, values::ReferenceValue)> {
    if from_value_id == to_value_id {
        return Err(AppError::ValidationFailed(vec![
            "La valeur source et la valeur cible doivent etre differentes.".into(),
        ]));
    }

    let from = load_bound_value(db, from_value_id).await?;
    let to = load_bound_value(db, to_value_id).await?;

    if from.domain_id != domain_id || to.domain_id != domain_id {
        return Err(AppError::ValidationFailed(vec![
            "Les valeurs source/cible doivent appartenir au domaine demandé.".into(),
        ]));
    }

    if from.domain_id != to.domain_id {
        return Err(AppError::ValidationFailed(vec![
            "Les valeurs source et cible doivent appartenir au même domaine.".into(),
        ]));
    }

    if !to.value.is_active {
        return Err(AppError::ValidationFailed(vec![
            "La valeur cible doit être active pour recevoir une migration.".into(),
        ]));
    }

    let domain = domains::get_reference_domain(db, domain_id).await?;
    Ok((domain, from.value, to.value))
}

fn normalize_limit(limit: i64) -> i64 {
    if limit <= 0 {
        50
    } else {
        limit.min(500)
    }
}

async fn insert_migration_row<C: ConnectionTrait>(
    db: &C,
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    reason_code: &str,
    actor_id: i64,
) -> AppResult<ReferenceValueMigration> {
    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO reference_value_migrations \
             (domain_id, from_value_id, to_value_id, reason_code, migrated_by_id, migrated_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            domain_id.into(),
            from_value_id.into(),
            to_value_id.into(),
            reason_code.to_string().into(),
            actor_id.into(),
            now.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, domain_id, from_value_id, to_value_id, reason_code, migrated_by_id, migrated_at \
             FROM reference_value_migrations \
             WHERE domain_id = ? AND from_value_id = ? AND to_value_id = ? \
             ORDER BY id DESC LIMIT 1",
            [domain_id.into(), from_value_id.into(), to_value_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "reference_value_migrations row missing after insert"
            ))
        })?;

    map_migration(&row)
}

async fn remap_usage_references<C: ConnectionTrait>(
    db: &C,
    domain: &domains::ReferenceDomain,
    from_value: &values::ReferenceValue,
    to_value: &values::ReferenceValue,
) -> AppResult<i64> {
    let mut total: i64 = 0;

    total += remap_equipment_classification_codes(db, domain, from_value, to_value).await?;
    total += remap_lookup_consumer_usage(db, domain, from_value, to_value).await?;

    Ok(total)
}

async fn remap_equipment_classification_codes<C: ConnectionTrait>(
    db: &C,
    domain: &domains::ReferenceDomain,
    from_value: &values::ReferenceValue,
    to_value: &values::ReferenceValue,
) -> AppResult<i64> {
    let domain_upper = domain.code.to_ascii_uppercase();
    let is_classification_domain = domain_upper.starts_with("EQUIPMENT")
        && (domain_upper.contains("CLASS")
            || domain_upper.contains("FAMILY")
            || domain_upper.contains("CLASSIFICATION"));

    if !is_classification_domain {
        return Ok(0);
    }

    let now = Utc::now().to_rfc3339();
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_classes \
             SET code = ?, updated_at = ? \
             WHERE code = ? AND deleted_at IS NULL \
               AND NOT EXISTS ( \
                   SELECT 1 FROM equipment_classes ec2 \
                   WHERE ec2.code = ? AND ec2.deleted_at IS NULL \
               )",
            [
                to_value.code.clone().into(),
                now.into(),
                from_value.code.clone().into(),
                to_value.code.clone().into(),
            ],
        ))
        .await?;

    Ok(result.rows_affected() as i64)
}

async fn remap_lookup_consumer_usage<C: ConnectionTrait>(
    db: &C,
    domain: &domains::ReferenceDomain,
    from_value: &values::ReferenceValue,
    to_value: &values::ReferenceValue,
) -> AppResult<i64> {
    let mut total: i64 = 0;
    let domain_key = domain.code.to_ascii_lowercase().replace('_', ".");

    let from_lookup_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id AS lookup_id \
             FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE LOWER(ld.domain_key) = ? \
               AND lv.code = ? \
               AND lv.deleted_at IS NULL \
               AND ld.deleted_at IS NULL \
             LIMIT 1",
            [domain_key.clone().into(), from_value.code.clone().into()],
        ))
        .await?;

    let to_lookup_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id AS lookup_id \
             FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE LOWER(ld.domain_key) = ? \
               AND lv.code = ? \
               AND lv.deleted_at IS NULL \
               AND ld.deleted_at IS NULL \
             LIMIT 1",
            [domain_key.into(), to_value.code.clone().into()],
        ))
        .await?;

    let from_lookup_id = from_lookup_row
        .as_ref()
        .and_then(|r| r.try_get::<i64>("", "lookup_id").ok());
    let to_lookup_id = to_lookup_row
        .as_ref()
        .and_then(|r| r.try_get::<i64>("", "lookup_id").ok());

    match (from_lookup_id, to_lookup_id) {
        (Some(from_id), Some(to_id)) => {
            // Remap known FK usage (equipment criticality).
            let eq = db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE equipment \
                     SET criticality_value_id = ? \
                     WHERE criticality_value_id = ? AND deleted_at IS NULL",
                    [to_id.into(), from_id.into()],
                ))
                .await?;
            total += eq.rows_affected() as i64;

            // Keep aliases attached to the surviving lookup value.
            let aliases = db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE lookup_value_aliases SET value_id = ? WHERE value_id = ?",
                    [to_id.into(), from_id.into()],
                ))
                .await?;
            total += aliases.rows_affected() as i64;

            // Soft-retire the source lookup value so consumers stop picking it.
            let now = Utc::now().to_rfc3339();
            let deactivate = db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE lookup_values \
                     SET is_active = 0, updated_at = ?, row_version = row_version + 1 \
                     WHERE id = ? AND is_active = 1",
                    [now.into(), from_id.into()],
                ))
                .await?;
            total += deactivate.rows_affected() as i64;
        }
        (Some(from_id), None) => {
            // Fallback: rename the existing lookup code when no target exists.
            let now = Utc::now().to_rfc3339();
            let rename = db
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE lookup_values \
                     SET code = ?, updated_at = ?, row_version = row_version + 1 \
                     WHERE id = ? AND deleted_at IS NULL \
                       AND NOT EXISTS ( \
                           SELECT 1 FROM lookup_values lv2 \
                           WHERE lv2.domain_id = lookup_values.domain_id \
                             AND lv2.code = ? \
                             AND lv2.deleted_at IS NULL \
                       )",
                    [
                        to_value.code.clone().into(),
                        now.into(),
                        from_id.into(),
                        to_value.code.clone().into(),
                    ],
                ))
                .await?;
            total += rename.rows_affected() as i64;
        }
        _ => {}
    }

    Ok(total)
}

async fn execute_migration_operation(
    db: &DatabaseConnection,
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    actor_id: i64,
    reason_code: &str,
) -> AppResult<ReferenceUsageMigrationResult> {
    let (domain, source_value, target_value) =
        validate_migration_pair(db, domain_id, from_value_id, to_value_id).await?;

    // Policy check before transaction: operation may deactivate source.
    if source_value.is_active {
        protected::assert_can_deactivate_value(db, source_value.id).await?;
    }

    let tx = db.begin().await?;

    let remapped_references =
        remap_usage_references(&tx, &domain, &source_value, &target_value).await?;

    let migration = insert_migration_row(
        &tx,
        domain_id,
        from_value_id,
        to_value_id,
        reason_code,
        actor_id,
    )
    .await?;

    let source_deactivated = if source_value.is_active {
        let result = tx
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE reference_values SET is_active = 0 WHERE id = ? AND is_active = 1",
                [from_value_id.into()],
            ))
            .await?;
        result.rows_affected() > 0
    } else {
        false
    };

    tx.commit().await?;

    let source_after = values::get_value(db, from_value_id).await?;
    let target_after = values::get_value(db, to_value_id).await?;

    Ok(ReferenceUsageMigrationResult {
        migration,
        source_value: source_after,
        target_value: target_after,
        remapped_references,
        source_deactivated,
    })
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Merge one source value into an active target value.
///
/// The operation records a migration map row, remaps known downstream usage,
/// and deactivates the source value after successful completion.
pub async fn merge_reference_values(
    db: &DatabaseConnection,
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceUsageMigrationResult> {
    execute_migration_operation(
        db,
        domain_id,
        from_value_id,
        to_value_id,
        actor_id,
        "merge",
    )
    .await
}

/// Migrate active usage from one value to another within the same domain.
///
/// This is a governed replacement operation that preserves traceability in
/// `reference_value_migrations` and retires the source value when allowed.
pub async fn migrate_reference_usage(
    db: &DatabaseConnection,
    domain_id: i64,
    from_value_id: i64,
    to_value_id: i64,
    actor_id: i64,
) -> AppResult<ReferenceUsageMigrationResult> {
    execute_migration_operation(
        db,
        domain_id,
        from_value_id,
        to_value_id,
        actor_id,
        "usage_migration",
    )
    .await
}

/// List recent value migration map rows for a domain.
pub async fn list_reference_migrations(
    db: &DatabaseConnection,
    domain_id: i64,
    limit: i64,
) -> AppResult<Vec<ReferenceValueMigration>> {
    // Ensures domain exists and returns NotFound if not.
    let _ = domains::get_reference_domain(db, domain_id).await?;

    let limit = normalize_limit(limit);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, domain_id, from_value_id, to_value_id, reason_code, migrated_by_id, migrated_at \
             FROM reference_value_migrations \
             WHERE domain_id = ? \
             ORDER BY id DESC \
             LIMIT ?",
            [domain_id.into(), limit.into()],
        ))
        .await?;

    rows.iter().map(map_migration).collect()
}

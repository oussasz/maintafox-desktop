//! Asset identity and classification service.
//!
//! Phase 2 - Sub-phase 02 - File 01 - Sprint S1.
//!
//! This service provides governed CRUD for asset identity on top of the
//! existing `equipment` table (migration 005, extended by migration 010).
//!
//! Column reconciliation:
//!   roadmap field        → DB column
//!   ─────────────────────────────────────────
//!   asset_code           → equipment.asset_id_code
//!   asset_name           → equipment.name
//!   class_code           → resolved via equipment_classes (equipment.class_id FK)
//!   family_code          → resolved via parent of equipment_classes
//!   criticality_code     → resolved via lookup_values (equipment.criticality_value_id FK)
//!   status_code          → equipment.lifecycle_status
//!   org_node_id          → equipment.installed_at_node_id
//!   commissioned_at      → equipment.commissioning_date
//!   decommissioned_at    → equipment.decommissioned_at (added in migration 010)
//!   maintainable_boundary→ equipment.maintainable_boundary (added in migration 010)

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Complete asset identity record for reads. Codes are resolved via JOINs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub sync_id: String,
    pub asset_code: String,
    pub asset_name: String,
    pub class_id: Option<i64>,
    pub class_code: Option<String>,
    pub class_name: Option<String>,
    pub family_code: Option<String>,
    pub family_name: Option<String>,
    pub criticality_value_id: Option<i64>,
    pub criticality_code: Option<String>,
    pub status_code: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub maintainable_boundary: bool,
    pub org_node_id: Option<i64>,
    pub org_node_name: Option<String>,
    pub commissioned_at: Option<String>,
    pub decommissioned_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub row_version: i64,
}

/// Payload for creating an asset. Codes are resolved to FKs during validation.
#[derive(Debug, Deserialize)]
pub struct CreateAssetPayload {
    pub asset_code: String,
    pub asset_name: String,
    pub class_code: String,
    pub family_code: Option<String>,
    pub criticality_code: String,
    pub status_code: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub maintainable_boundary: bool,
    pub org_node_id: i64,
    pub commissioned_at: Option<String>,
}

/// Payload for updating asset identity fields. Only provided fields are changed.
#[derive(Debug, Deserialize)]
pub struct UpdateAssetIdentityPayload {
    pub asset_name: Option<String>,
    pub class_code: Option<String>,
    pub family_code: Option<Option<String>>,
    pub criticality_code: Option<String>,
    pub status_code: Option<String>,
    pub manufacturer: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub serial_number: Option<Option<String>>,
    pub maintainable_boundary: Option<bool>,
    pub commissioned_at: Option<Option<String>>,
    pub decommissioned_at: Option<Option<String>>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "equipment row decode failed for column '{column}': {e}"
    ))
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

/// SELECT columns for the asset query. Resolves class, family, criticality,
/// and org node names via JOINs for the read-side `Asset` struct.
pub(crate) const ASSET_SELECT: &str = r"
    e.id,
    e.sync_id,
    e.asset_id_code    AS asset_code,
    e.name             AS asset_name,
    e.class_id,
    ec.code            AS class_code,
    ec.name            AS class_name,
    ef.code            AS family_code,
    ef.name            AS family_name,
    e.criticality_value_id,
    lv.code            AS criticality_code,
    e.lifecycle_status AS status_code,
    e.manufacturer,
    e.model,
    e.serial_number,
    e.maintainable_boundary,
    e.installed_at_node_id AS org_node_id,
    n.name             AS org_node_name,
    e.commissioning_date AS commissioned_at,
    e.decommissioned_at,
    e.created_at,
    e.updated_at,
    e.deleted_at,
    e.row_version
";

/// Standard FROM/JOIN clause for asset queries.
pub(crate) const ASSET_FROM: &str = r"
    FROM equipment e
    LEFT JOIN equipment_classes ec ON ec.id = e.class_id
    LEFT JOIN equipment_classes ef ON ef.id = ec.parent_id
    LEFT JOIN lookup_values lv     ON lv.id = e.criticality_value_id
    LEFT JOIN org_nodes n          ON n.id  = e.installed_at_node_id
";

pub(crate) fn map_asset(row: &QueryResult) -> AppResult<Asset> {
    Ok(Asset {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        asset_code: row
            .try_get::<String>("", "asset_code")
            .map_err(|e| decode_err("asset_code", e))?,
        asset_name: row
            .try_get::<String>("", "asset_name")
            .map_err(|e| decode_err("asset_name", e))?,
        class_id: row
            .try_get::<Option<i64>>("", "class_id")
            .map_err(|e| decode_err("class_id", e))?,
        class_code: row
            .try_get::<Option<String>>("", "class_code")
            .map_err(|e| decode_err("class_code", e))?,
        class_name: row
            .try_get::<Option<String>>("", "class_name")
            .map_err(|e| decode_err("class_name", e))?,
        family_code: row
            .try_get::<Option<String>>("", "family_code")
            .map_err(|e| decode_err("family_code", e))?,
        family_name: row
            .try_get::<Option<String>>("", "family_name")
            .map_err(|e| decode_err("family_name", e))?,
        criticality_value_id: row
            .try_get::<Option<i64>>("", "criticality_value_id")
            .map_err(|e| decode_err("criticality_value_id", e))?,
        criticality_code: row
            .try_get::<Option<String>>("", "criticality_code")
            .map_err(|e| decode_err("criticality_code", e))?,
        status_code: row
            .try_get::<String>("", "status_code")
            .map_err(|e| decode_err("status_code", e))?,
        manufacturer: row
            .try_get::<Option<String>>("", "manufacturer")
            .map_err(|e| decode_err("manufacturer", e))?,
        model: row
            .try_get::<Option<String>>("", "model")
            .map_err(|e| decode_err("model", e))?,
        serial_number: row
            .try_get::<Option<String>>("", "serial_number")
            .map_err(|e| decode_err("serial_number", e))?,
        maintainable_boundary: i64_to_bool(
            row.try_get::<i64>("", "maintainable_boundary")
                .map_err(|e| decode_err("maintainable_boundary", e))?,
        ),
        org_node_id: row
            .try_get::<Option<i64>>("", "org_node_id")
            .map_err(|e| decode_err("org_node_id", e))?,
        org_node_name: row
            .try_get::<Option<String>>("", "org_node_name")
            .map_err(|e| decode_err("org_node_name", e))?,
        commissioned_at: row
            .try_get::<Option<String>>("", "commissioned_at")
            .map_err(|e| decode_err("commissioned_at", e))?,
        decommissioned_at: row
            .try_get::<Option<String>>("", "decommissioned_at")
            .map_err(|e| decode_err("decommissioned_at", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
        deleted_at: row
            .try_get::<Option<String>>("", "deleted_at")
            .map_err(|e| decode_err("deleted_at", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

// ─── Internal validation helpers ──────────────────────────────────────────────

/// Resolve an equipment class code to its row id.
/// Returns `(class_id, parent_id)` where `parent_id` is the family-level class.
pub(crate) async fn resolve_class_code(
    db: &impl ConnectionTrait,
    class_code: &str,
) -> AppResult<(i64, Option<i64>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_id FROM equipment_classes \
             WHERE code = ? AND is_active = 1 AND deleted_at IS NULL",
            [class_code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "Classe d'équipement '{class_code}' introuvable ou inactive."
            )])
        })?;
    let id: i64 = row
        .try_get("", "id")
        .map_err(|e| decode_err("class.id", e))?;
    let parent_id: Option<i64> = row
        .try_get("", "parent_id")
        .map_err(|e| decode_err("class.parent_id", e))?;
    Ok((id, parent_id))
}

/// Validate that a family_code exists and is the parent of the resolved class.
/// If the class has no parent (it IS top-level), the family code is optional
/// and this function returns Ok if family_code matches the class itself.
pub(crate) async fn validate_family_code(
    db: &impl ConnectionTrait,
    family_code: &str,
    class_id: i64,
    class_parent_id: Option<i64>,
) -> AppResult<()> {
    let expected_parent_id = match class_parent_id {
        Some(pid) => pid,
        None => {
            // Top-level class — family_code should match the class itself
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT code FROM equipment_classes WHERE id = ?",
                    [class_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("class row missing")))?;
            let code: String = row
                .try_get("", "code")
                .map_err(|e| decode_err("code", e))?;
            if code != family_code {
                return Err(AppError::ValidationFailed(vec![format!(
                    "La classe '{code}' est de niveau racine; \
                     le code famille '{family_code}' ne correspond pas."
                )]));
            }
            return Ok(());
        }
    };

    // Verify the parent's code matches family_code
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM equipment_classes \
             WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [expected_parent_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "La classe parente (famille) est inactive ou supprimée.".into(),
            ])
        })?;
    let parent_code: String = row
        .try_get("", "code")
        .map_err(|e| decode_err("parent.code", e))?;
    if parent_code != family_code {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le code famille '{family_code}' ne correspond pas à la famille \
             '{parent_code}' de la classe sélectionnée."
        )]));
    }
    Ok(())
}

/// Resolve a criticality code to its lookup_values.id within the
/// `equipment.criticality` domain.
pub(crate) async fn resolve_criticality_code(
    db: &impl ConnectionTrait,
    code: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'equipment.criticality' \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "Code criticité '{code}' introuvable dans le domaine 'equipment.criticality'."
            )])
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| decode_err("criticality.id", e))
}

/// Validate that a status code exists in the `equipment.lifecycle_status` domain.
pub(crate) async fn validate_status_code(
    db: &impl ConnectionTrait,
    code: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'equipment.lifecycle_status' \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Code statut '{code}' introuvable dans le domaine 'equipment.lifecycle_status'."
        )]));
    }
    Ok(())
}

/// Validate that an org node id references an active, non-deleted org node.
pub(crate) async fn assert_org_node_active(
    db: &impl ConnectionTrait,
    org_node_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [org_node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".into(),
            id: org_node_id.to_string(),
        })?;
    let status: String = row
        .try_get("", "status")
        .map_err(|e| decode_err("org_node.status", e))?;
    if status != "active" {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le noeud organisationnel {org_node_id} n'est pas actif (statut: {status})."
        )]));
    }
    Ok(())
}

/// Validate that an asset code is uppercase, non-empty, and correctly formatted.
pub(crate) fn validate_asset_code(code: &str) -> AppResult<String> {
    let trimmed = code.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le code équipement ne peut pas être vide.".into(),
        ]));
    }
    if trimmed.len() > 64 {
        return Err(AppError::ValidationFailed(vec![
            "Le code équipement ne peut pas dépasser 64 caractères.".into(),
        ]));
    }
    // Must be uppercase + digits + dashes + underscores
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(AppError::ValidationFailed(vec![
            "Le code équipement ne peut contenir que des majuscules, chiffres, tirets et tirets bas.".into(),
        ]));
    }
    Ok(trimmed)
}

/// Check that asset_id_code is unique among non-deleted equipment rows.
pub(crate) async fn assert_asset_code_unique(
    db: &impl ConnectionTrait,
    code: &str,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    let (sql, binds): (&str, Vec<sea_orm::Value>) = if let Some(eid) = exclude_id {
        (
            "SELECT COUNT(*) AS cnt FROM equipment \
             WHERE asset_id_code = ? AND deleted_at IS NULL AND id != ?",
            vec![code.into(), eid.into()],
        )
    } else {
        (
            "SELECT COUNT(*) AS cnt FROM equipment \
             WHERE asset_id_code = ? AND deleted_at IS NULL",
            vec![code.into()],
        )
    };
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Le code équipement '{code}' existe déjà."
        )]));
    }
    Ok(())
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List assets with optional filters. Returns resolved codes via JOINs.
///
/// # Arguments
/// - `status_filter` — optional lifecycle status code to filter by
/// - `org_node_filter` — optional org node id to filter by
/// - `query` — optional text search (asset_code, asset_name, serial_number)
/// - `limit` — max rows (capped at 200)
pub async fn list_assets(
    db: &DatabaseConnection,
    status_filter: Option<String>,
    org_node_filter: Option<i64>,
    query: Option<String>,
    limit: Option<u64>,
) -> AppResult<Vec<Asset>> {
    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref status) = status_filter {
        where_clauses.push("e.lifecycle_status = ?".to_string());
        binds.push(status.clone().into());
    }
    if let Some(node_id) = org_node_filter {
        where_clauses.push("e.installed_at_node_id = ?".to_string());
        binds.push(node_id.into());
    }
    if let Some(ref q) = query {
        where_clauses.push(
            "(e.asset_id_code LIKE ? OR e.name LIKE ? OR e.serial_number LIKE ?)".to_string(),
        );
        let pattern = format!("%{q}%");
        binds.push(pattern.clone().into());
        binds.push(pattern.clone().into());
        binds.push(pattern.into());
    }

    let where_sql = where_clauses.join(" AND ");
    let row_limit = limit.unwrap_or(100).min(200);

    let sql = format!(
        "SELECT {ASSET_SELECT} {ASSET_FROM} \
         WHERE {where_sql} \
         ORDER BY e.asset_id_code ASC \
         LIMIT {row_limit}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    rows.iter().map(map_asset).collect()
}

/// Get a single asset by id with resolved classification and org codes.
pub async fn get_asset_by_id(
    db: &DatabaseConnection,
    asset_id: i64,
) -> AppResult<Asset> {
    let sql = format!(
        "SELECT {ASSET_SELECT} {ASSET_FROM} \
         WHERE e.id = ? AND e.deleted_at IS NULL"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    map_asset(&row)
}

/// Create a new governed asset. Runs inside a transaction.
///
/// Validation:
///   - `asset_code` must be uppercase, unique among non-deleted
///   - `class_code` must reference an active equipment class
///   - `family_code` (if provided) must match the parent of the resolved class
///   - `criticality_code` must exist in `equipment.criticality` domain
///   - `status_code` must exist in `equipment.lifecycle_status` domain
///   - `org_node_id` must reference an active org node
///   - if status = DECOMMISSIONED, `decommissioned_at` is implicitly set to now
pub async fn create_asset(
    db: &DatabaseConnection,
    payload: CreateAssetPayload,
    _actor_id: i32,
) -> AppResult<Asset> {
    // ── Format and validate asset code ────────────────────────────────────
    let asset_code = validate_asset_code(&payload.asset_code)?;

    let asset_name = payload.asset_name.trim().to_string();
    if asset_name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le nom de l'équipement ne peut pas être vide.".into(),
        ]));
    }

    let txn = db.begin().await?;

    // ── Code uniqueness ──────────────────────────────────────────────────
    assert_asset_code_unique(&txn, &asset_code, None).await?;

    // ── Org node linkage guard ───────────────────────────────────────────
    assert_org_node_active(&txn, payload.org_node_id).await?;

    // ── Classification resolution ────────────────────────────────────────
    let (class_id, class_parent_id) = resolve_class_code(&txn, &payload.class_code).await?;

    if let Some(ref family_code) = payload.family_code {
        validate_family_code(&txn, family_code, class_id, class_parent_id).await?;
    }

    // ── Criticality resolution ───────────────────────────────────────────
    let criticality_value_id =
        resolve_criticality_code(&txn, &payload.criticality_code).await?;

    // ── Status validation ────────────────────────────────────────────────
    validate_status_code(&txn, &payload.status_code).await?;

    // ── Decommission date guard ──────────────────────────────────────────
    let decommissioned_at: Option<String> = if payload.status_code == "DECOMMISSIONED" {
        Some(Utc::now().to_rfc3339())
    } else {
        None
    };

    // ── Maintainable boundary guard ──────────────────────────────────────
    // A non-maintainable asset should not be marked in-service unless the
    // class policy allows it. For now, warn-level validation only.
    // (Full class-policy enforcement deferred to File 04.)

    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO equipment
          (sync_id, asset_id_code, name, class_id,
           lifecycle_status, criticality_value_id,
           installed_at_node_id, manufacturer, model, serial_number,
           maintainable_boundary, commissioning_date, decommissioned_at,
           created_at, updated_at, row_version)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.clone().into(),
            asset_code.into(),
            asset_name.into(),
            class_id.into(),
            payload.status_code.into(),
            criticality_value_id.into(),
            payload.org_node_id.into(),
            payload.manufacturer.into(),
            payload.model.into(),
            payload.serial_number.into(),
            i64::from(payload.maintainable_boundary).into(),
            payload.commissioned_at.into(),
            decommissioned_at.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve inserted id via sync_id
    let id_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE sync_id = ?",
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "asset created but not found after insert"
            ))
        })?;
    let asset_id: i64 = id_row
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    // Fetch the full asset with resolved codes
    let sql = format!(
        "SELECT {ASSET_SELECT} {ASSET_FROM} WHERE e.id = ?"
    );
    let asset_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "asset {asset_id} not found after insert"
            ))
        })?;
    let asset = map_asset(&asset_row)?;

    txn.commit().await?;

    tracing::info!(asset_id, "asset created");
    Ok(asset)
}

/// Update the identity fields of an existing asset.
///
/// Uses optimistic concurrency control via `expected_row_version`.
/// Only provided (Some) fields are updated; None fields are left unchanged.
pub async fn update_asset_identity(
    db: &DatabaseConnection,
    asset_id: i64,
    payload: UpdateAssetIdentityPayload,
    expected_row_version: i64,
    _actor_id: i32,
) -> AppResult<Asset> {
    let txn = db.begin().await?;

    // ── Fetch current row and verify row_version ─────────────────────────
    let current = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, lifecycle_status, asset_id_code FROM equipment \
             WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;

    let current_version: i64 = current
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if current_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "Conflit de version : version attendue {expected_row_version}, \
             version actuelle {current_version}. L'enregistrement a été modifié."
        )]));
    }

    // ── Build dynamic SET clause ─────────────────────────────────────────
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(ref name) = payload.asset_name {
        let t = name.trim().to_string();
        if t.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Le nom de l'équipement ne peut pas être vide.".into(),
            ]));
        }
        sets.push("name = ?".to_string());
        binds.push(t.into());
    }

    if let Some(ref class_code) = payload.class_code {
        let (cid, parent_id) = resolve_class_code(&txn, class_code).await?;
        if let Some(Some(ref family_code)) = payload.family_code {
            validate_family_code(&txn, family_code, cid, parent_id).await?;
        }
        sets.push("class_id = ?".to_string());
        binds.push(cid.into());
    }

    if let Some(ref crit_code) = payload.criticality_code {
        let crit_id = resolve_criticality_code(&txn, crit_code).await?;
        sets.push("criticality_value_id = ?".to_string());
        binds.push(crit_id.into());
    }

    if let Some(ref status_code) = payload.status_code {
        validate_status_code(&txn, status_code).await?;
        sets.push("lifecycle_status = ?".to_string());
        binds.push(status_code.clone().into());

        // Auto-set decommissioned_at if transitioning to DECOMMISSIONED
        if status_code == "DECOMMISSIONED" {
            let has_explicit_decom = matches!(payload.decommissioned_at, Some(Some(_)));
            if !has_explicit_decom {
                sets.push("decommissioned_at = ?".to_string());
                binds.push(Utc::now().to_rfc3339().into());
            }
        }
    }

    if let Some(ref mfg) = payload.manufacturer {
        sets.push("manufacturer = ?".to_string());
        binds.push(mfg.clone().into());
    }
    if let Some(ref mdl) = payload.model {
        sets.push("model = ?".to_string());
        binds.push(mdl.clone().into());
    }
    if let Some(ref sn) = payload.serial_number {
        sets.push("serial_number = ?".to_string());
        binds.push(sn.clone().into());
    }
    if let Some(mb) = payload.maintainable_boundary {
        sets.push("maintainable_boundary = ?".to_string());
        binds.push(i64::from(mb).into());
    }
    if let Some(ref com_at) = payload.commissioned_at {
        sets.push("commissioning_date = ?".to_string());
        binds.push(com_at.clone().into());
    }
    if let Some(ref decom_at) = payload.decommissioned_at {
        sets.push("decommissioned_at = ?".to_string());
        binds.push(decom_at.clone().into());
    }

    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Aucun champ à mettre à jour.".into(),
        ]));
    }

    // Always bump version and timestamp
    let now = Utc::now().to_rfc3339();
    sets.push("updated_at = ?".to_string());
    binds.push(now.into());
    sets.push("row_version = row_version + 1".to_string());

    // WHERE clause
    binds.push(asset_id.into());
    binds.push(expected_row_version.into());

    let set_sql = sets.join(", ");
    let update_sql = format!(
        "UPDATE equipment SET {set_sql} WHERE id = ? AND row_version = ?"
    );

    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &update_sql,
            binds,
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Mise à jour impossible : version obsolète ou enregistrement supprimé.".into(),
        ]));
    }

    // Fetch updated asset with resolved codes
    let select_sql = format!(
        "SELECT {ASSET_SELECT} {ASSET_FROM} WHERE e.id = ?"
    );
    let asset_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            select_sql,
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    let asset = map_asset(&asset_row)?;

    txn.commit().await?;

    tracing::info!(asset_id, "asset identity updated");
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_asset_code_rejects_empty() {
        assert!(validate_asset_code("").is_err());
        assert!(validate_asset_code("   ").is_err());
    }

    #[test]
    fn validate_asset_code_rejects_lowercase() {
        assert!(validate_asset_code("pmp-1001").is_err());
    }

    #[test]
    fn validate_asset_code_accepts_valid() {
        assert_eq!(validate_asset_code("PMP-1001").unwrap(), "PMP-1001");
        assert_eq!(validate_asset_code("CONV_A02").unwrap(), "CONV_A02");
    }

    #[test]
    fn validate_asset_code_rejects_over_64_chars() {
        let long = "A".repeat(65);
        assert!(validate_asset_code(&long).is_err());
    }

    #[test]
    fn validate_asset_code_trims_whitespace() {
        assert_eq!(validate_asset_code("  PMP-1001  ").unwrap(), "PMP-1001");
    }

    #[test]
    fn i64_bool_roundtrip() {
        assert!(i64_to_bool(1));
        assert!(!i64_to_bool(0));
        assert!(i64_to_bool(99));
    }
}

//! Asset search service.
//!
//! Phase 2 - Sub-phase 02 - File 03 - Sprint S1.
//!
//! Provides domain-aware search across asset identity fields with multi-criteria
//! filtering. Search ranks asset code first, then name, org location, class,
//! family, status, and external IDs rather than plain text.
//!
//! The search result is an enriched DTO that includes parent asset context,
//! primary meter summary, and external ID count — giving downstream consumers
//! a quick snapshot without separate round-trips.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Inbound filter payload deserialized from the IPC call.
#[derive(Debug, Deserialize)]
pub struct AssetSearchFilters {
    pub query: Option<String>,
    pub class_codes: Option<Vec<String>>,
    pub family_codes: Option<Vec<String>>,
    pub status_codes: Option<Vec<String>>,
    pub org_node_ids: Option<Vec<i64>>,
    pub include_decommissioned: Option<bool>,
    pub limit: Option<u64>,
}

/// Enriched search result DTO for each matching asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSearchResult {
    pub id: i64,
    pub sync_id: String,
    pub asset_code: String,
    pub asset_name: String,
    pub class_code: Option<String>,
    pub class_name: Option<String>,
    pub family_code: Option<String>,
    pub family_name: Option<String>,
    pub criticality_code: Option<String>,
    pub status_code: String,
    pub org_node_id: Option<i64>,
    pub org_node_name: Option<String>,
    // Parent asset context (from hierarchy)
    pub parent_asset_id: Option<i64>,
    pub parent_asset_code: Option<String>,
    pub parent_asset_name: Option<String>,
    // Primary meter summary
    pub primary_meter_name: Option<String>,
    pub primary_meter_reading: Option<f64>,
    pub primary_meter_unit: Option<String>,
    pub primary_meter_last_read_at: Option<String>,
    // External ID count
    pub external_id_count: i64,
    pub row_version: i64,
}

/// Lightweight suggestion item for typeahead / autocomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSuggestion {
    pub id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub status_code: String,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "search result decode failed for column '{column}': {e}"
    ))
}

fn map_search_result(row: &QueryResult) -> AppResult<AssetSearchResult> {
    Ok(AssetSearchResult {
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
        criticality_code: row
            .try_get::<Option<String>>("", "criticality_code")
            .map_err(|e| decode_err("criticality_code", e))?,
        status_code: row
            .try_get::<String>("", "status_code")
            .map_err(|e| decode_err("status_code", e))?,
        org_node_id: row
            .try_get::<Option<i64>>("", "org_node_id")
            .map_err(|e| decode_err("org_node_id", e))?,
        org_node_name: row
            .try_get::<Option<String>>("", "org_node_name")
            .map_err(|e| decode_err("org_node_name", e))?,
        parent_asset_id: row
            .try_get::<Option<i64>>("", "parent_asset_id")
            .map_err(|e| decode_err("parent_asset_id", e))?,
        parent_asset_code: row
            .try_get::<Option<String>>("", "parent_asset_code")
            .map_err(|e| decode_err("parent_asset_code", e))?,
        parent_asset_name: row
            .try_get::<Option<String>>("", "parent_asset_name")
            .map_err(|e| decode_err("parent_asset_name", e))?,
        primary_meter_name: row
            .try_get::<Option<String>>("", "primary_meter_name")
            .map_err(|e| decode_err("primary_meter_name", e))?,
        primary_meter_reading: row
            .try_get::<Option<f64>>("", "primary_meter_reading")
            .map_err(|e| decode_err("primary_meter_reading", e))?,
        primary_meter_unit: row
            .try_get::<Option<String>>("", "primary_meter_unit")
            .map_err(|e| decode_err("primary_meter_unit", e))?,
        primary_meter_last_read_at: row
            .try_get::<Option<String>>("", "primary_meter_last_read_at")
            .map_err(|e| decode_err("primary_meter_last_read_at", e))?,
        external_id_count: row
            .try_get::<i64>("", "external_id_count")
            .map_err(|e| decode_err("external_id_count", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_suggestion(row: &QueryResult) -> AppResult<AssetSuggestion> {
    Ok(AssetSuggestion {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        asset_code: row
            .try_get::<String>("", "asset_code")
            .map_err(|e| decode_err("asset_code", e))?,
        asset_name: row
            .try_get::<String>("", "asset_name")
            .map_err(|e| decode_err("asset_name", e))?,
        status_code: row
            .try_get::<String>("", "status_code")
            .map_err(|e| decode_err("status_code", e))?,
    })
}

// ─── SQL fragments ────────────────────────────────────────────────────────────

/// Enriched SELECT for search results. Extends the identity SELECT with parent
/// asset context, primary meter summary, and external ID count via sub-queries.
const SEARCH_SELECT: &str = r"
    e.id,
    e.sync_id,
    e.asset_id_code         AS asset_code,
    e.name                  AS asset_name,
    ec.code                 AS class_code,
    ec.name                 AS class_name,
    ef.code                 AS family_code,
    ef.name                 AS family_name,
    COALESCE(rs_crit.code, lv.code) AS criticality_code,
    COALESCE(rs_stat.code, e.lifecycle_status) AS status_code,
    e.installed_at_node_id  AS org_node_id,
    n.name                  AS org_node_name,
    -- Parent asset (first active parent from hierarchy)
    ph.parent_equipment_id  AS parent_asset_id,
    pe.asset_id_code        AS parent_asset_code,
    pe.name                 AS parent_asset_name,
    -- Primary meter summary
    pm.name                 AS primary_meter_name,
    pm.current_reading      AS primary_meter_reading,
    pm.unit                 AS primary_meter_unit,
    pm.last_read_at         AS primary_meter_last_read_at,
    -- External ID count
    COALESCE(xid.ext_count, 0) AS external_id_count,
    e.row_version
";

const SEARCH_FROM: &str = r"
    FROM equipment e
    LEFT JOIN equipment_classes ec ON ec.id = e.class_id
    LEFT JOIN equipment_classes ef ON ef.id = ec.parent_id
    LEFT JOIN lookup_values lv     ON lv.id = e.criticality_value_id
    LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id
    LEFT JOIN reference_values rs_stat ON rs_stat.id = e.equipment_status_ref_id
    LEFT JOIN org_nodes n          ON n.id  = e.installed_at_node_id
    -- First active parent (effective_to IS NULL means currently active)
    LEFT JOIN equipment_hierarchy ph
        ON ph.child_equipment_id = e.id AND ph.effective_to IS NULL
    LEFT JOIN equipment pe
        ON pe.id = ph.parent_equipment_id AND pe.deleted_at IS NULL
    -- Primary meter (is_primary = 1, is_active = 1)
    LEFT JOIN equipment_meters pm
        ON pm.equipment_id = e.id AND pm.is_primary = 1 AND pm.is_active = 1
    -- External ID count sub-query
    LEFT JOIN (
        SELECT asset_id, COUNT(*) AS ext_count
        FROM asset_external_ids
        WHERE valid_to IS NULL
        GROUP BY asset_id
    ) xid ON xid.asset_id = e.id
";

// ─── Service functions ────────────────────────────────────────────────────────

/// Domain-aware asset search with multi-criteria filtering.
///
/// Search ranking: asset code exact match is ordered first (via CASE expression),
/// then code prefix, then name/serial partial match.
///
/// Filters:
///   - `query` — searches asset_code, name, serial_number, and external_ids
///   - `class_codes` — restrict to specific equipment class codes
///   - `family_codes` — restrict to specific family (parent class) codes
///   - `status_codes` — restrict to specific lifecycle status codes
///   - `org_node_ids` — restrict to specific org nodes
///   - `include_decommissioned` — when false (default), excludes DECOMMISSIONED
///   - `limit` — max rows returned (capped at 200)
pub async fn search_assets(
    db: &DatabaseConnection,
    filters: AssetSearchFilters,
) -> AppResult<Vec<AssetSearchResult>> {
    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    // ── Decommissioned filter ─────────────────────────────────────────────
    if !filters.include_decommissioned.unwrap_or(false) {
        where_clauses.push("e.lifecycle_status != 'DECOMMISSIONED'".to_string());
    }

    // ── Text query (domain-aware: code, name, serial, external IDs) ──────
    if let Some(ref q) = filters.query {
        let trimmed = q.trim();
        if !trimmed.is_empty() {
            where_clauses.push(
                "(e.asset_id_code LIKE ? OR e.name LIKE ? OR e.serial_number LIKE ? \
                 OR e.id IN (SELECT asset_id FROM asset_external_ids WHERE external_id LIKE ? AND valid_to IS NULL))"
                    .to_string(),
            );
            let pattern = format!("%{trimmed}%");
            binds.push(pattern.clone().into());
            binds.push(pattern.clone().into());
            binds.push(pattern.clone().into());
            binds.push(pattern.into());
        }
    }

    // ── Class code filter ─────────────────────────────────────────────────
    if let Some(ref codes) = filters.class_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("ec.code IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    // ── Family code filter ────────────────────────────────────────────────
    if let Some(ref codes) = filters.family_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("ef.code IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    // ── Status code filter ────────────────────────────────────────────────
    if let Some(ref codes) = filters.status_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("e.lifecycle_status IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    // ── Org node filter ───────────────────────────────────────────────────
    if let Some(ref ids) = filters.org_node_ids {
        if !ids.is_empty() {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("e.installed_at_node_id IN ({placeholders})"));
            for &node_id in ids {
                binds.push(node_id.into());
            }
        }
    }

    let where_sql = where_clauses.join(" AND ");
    let row_limit = filters.limit.unwrap_or(100).min(200);

    // ── Order: exact code match first, then code prefix, then name ────────
    let order_sql = if let Some(ref q) = filters.query {
        let trimmed = q.trim();
        if !trimmed.is_empty() {
            // Bind the query for CASE ordering expressions
            binds.push(trimmed.to_uppercase().into());
            binds.push(format!("{}%", trimmed.to_uppercase()).into());
            "ORDER BY \
                CASE WHEN UPPER(e.asset_id_code) = UPPER(?) THEN 0 \
                     WHEN UPPER(e.asset_id_code) LIKE UPPER(?) THEN 1 \
                     ELSE 2 END, \
                e.asset_id_code ASC"
                .to_string()
        } else {
            "ORDER BY e.asset_id_code ASC".to_string()
        }
    } else {
        "ORDER BY e.asset_id_code ASC".to_string()
    };

    let sql = format!(
        "SELECT {SEARCH_SELECT} {SEARCH_FROM} WHERE {where_sql} {order_sql} LIMIT {row_limit}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    rows.iter().map(map_search_result).collect()
}

/// Suggest asset codes matching a prefix. Returns lightweight entries for
/// typeahead / autocomplete controls.
pub async fn suggest_asset_codes(
    db: &DatabaseConnection,
    prefix: &str,
    limit: Option<u64>,
) -> AppResult<Vec<AssetSuggestion>> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let pattern = format!("{trimmed}%");
    let row_limit = limit.unwrap_or(10).min(50);

    let sql = format!(
        "SELECT e.id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                e.lifecycle_status AS status_code \
         FROM equipment e \
         WHERE e.deleted_at IS NULL \
           AND e.asset_id_code LIKE ? \
         ORDER BY e.asset_id_code ASC \
         LIMIT {row_limit}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [pattern.into()],
        ))
        .await?;

    rows.iter().map(map_suggestion).collect()
}

/// Suggest asset names matching a partial string. Returns lightweight entries
/// for typeahead / autocomplete controls.
pub async fn suggest_asset_names(
    db: &DatabaseConnection,
    partial: &str,
    limit: Option<u64>,
) -> AppResult<Vec<AssetSuggestion>> {
    let trimmed = partial.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let pattern = format!("%{trimmed}%");
    let row_limit = limit.unwrap_or(10).min(50);

    let sql = format!(
        "SELECT e.id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                e.lifecycle_status AS status_code \
         FROM equipment e \
         WHERE e.deleted_at IS NULL \
           AND e.name LIKE ? \
         ORDER BY e.name ASC \
         LIMIT {row_limit}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [pattern.into()],
        ))
        .await?;

    rows.iter().map(map_suggestion).collect()
}

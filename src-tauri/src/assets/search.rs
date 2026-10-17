//! Asset search service.
//!
//! Provides domain-aware search across asset identity fields with multi-criteria
//! filtering. Search is case- and accent-insensitive (French diacritics) and ranks
//! exact code matches first, then prefix, then partial name/serial/external ID.
//!
//! The search result is an enriched DTO that includes parent asset context,
//! primary meter summary, org path, and external ID count.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// KPI strip click-to-filter focus (`status_down` | `critical_at_risk` | `open_work` | `stale_meters`).
    #[serde(default)]
    pub registry_focus: Option<String>,
    /// Rows to skip (paged registry list).
    #[serde(default)]
    pub offset: Option<u64>,
    /// Canonical criticality classes (`A`..`D`).
    #[serde(default)]
    pub criticality_codes: Option<Vec<String>>,
    /// When true, `org_node_ids` also matches assets installed anywhere below those nodes.
    #[serde(default)]
    pub org_subtree: Option<bool>,
}

/// One page of the registry list and the number of assets matching the filters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSearchPage {
    pub items: Vec<AssetSearchResult>,
    pub total: i64,
}

/// Largest page the registry list may request.
pub const SEARCH_PAGE_MAX: u64 = 200;
/// Page size used when the caller gives none.
pub const SEARCH_PAGE_DEFAULT: u64 = 100;

/// Filters for the shared AssetPicker empty-query / typed suggestions.
#[derive(Debug, Deserialize)]
pub struct AssetPickerSuggestFilters {
    pub query: Option<String>,
    pub include_decommissioned: Option<bool>,
    pub limit: Option<u64>,
}

/// Enriched search result DTO for each matching asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSearchResult {
    pub id: i64,
    pub entity_sync_id: String,
    pub asset_code: String,
    pub asset_name: String,
    pub class_code: Option<String>,
    pub class_name: Option<String>,
    pub family_code: Option<String>,
    pub family_name: Option<String>,
    pub subfamily_code: Option<String>,
    pub subfamily_name: Option<String>,
    pub criticality_code: Option<String>,
    pub status_code: String,
    pub org_node_id: Option<i64>,
    pub org_node_name: Option<String>,
    /// Root → … → node display path (e.g. "Site / Zone / Line").
    pub org_path: Option<String>,
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

/// Picker suggestion payload: recent-for-user or global frequent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPickerSuggestions {
    /// `"recent"` or `"frequent"`.
    pub mode: String,
    pub items: Vec<AssetSearchResult>,
}

// ─── Accent / case fold ───────────────────────────────────────────────────────

/// Lowercase and strip Latin/French diacritics for accent-insensitive search.
pub fn fold_search_text(input: &str) -> String {
    let lower: String = input.chars().flat_map(char::to_lowercase).collect();
    let mut out = String::with_capacity(lower.len());
    for ch in lower.chars() {
        match ch {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => out.push('a'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            other => out.push(other),
        }
    }
    out
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "search result decode failed for column '{column}': {e}"
    ))
}

pub(crate) fn map_search_result(row: &QueryResult) -> AppResult<AssetSearchResult> {
    Ok(AssetSearchResult {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get::<String>("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
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
        subfamily_code: row
            .try_get::<Option<String>>("", "subfamily_code")
            .map_err(|e| decode_err("subfamily_code", e))?,
        subfamily_name: row
            .try_get::<Option<String>>("", "subfamily_name")
            .map_err(|e| decode_err("subfamily_name", e))?,
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
        org_path: None,
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
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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

/// Resolve org paths for the returned result set only (bounded by result count).
pub(crate) async fn enrich_org_paths(db: &DatabaseConnection, results: &mut [AssetSearchResult]) -> AppResult<()> {
    let mut node_ids: Vec<i64> = results.iter().filter_map(|r| r.org_node_id).collect();
    node_ids.sort_unstable();
    node_ids.dedup();
    if node_ids.is_empty() {
        return Ok(());
    }

    let placeholders = node_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let binds: Vec<sea_orm::Value> = node_ids.iter().map(|&id| id.into()).collect();

    let sql = format!(
        "WITH RECURSIVE chain AS (
            SELECT id AS leaf_id, id, parent_id, name, 0 AS depth
            FROM org_nodes
            WHERE id IN ({placeholders}) AND deleted_at IS NULL
            UNION ALL
            SELECT c.leaf_id, n.id, n.parent_id, n.name, c.depth + 1
            FROM org_nodes n
            INNER JOIN chain c ON n.id = c.parent_id
            WHERE n.deleted_at IS NULL AND c.depth < 32
         )
         SELECT leaf_id, name, depth FROM chain
         ORDER BY leaf_id ASC, depth DESC"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut path_parts: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        let leaf_id: i64 = row.try_get("", "leaf_id").map_err(|e| decode_err("leaf_id", e))?;
        let name: String = row.try_get("", "name").map_err(|e| decode_err("name", e))?;
        path_parts.entry(leaf_id).or_default().push(name);
    }

    let path_map: HashMap<i64, String> = path_parts
        .into_iter()
        .map(|(id, parts)| (id, parts.join(" / ")))
        .collect();

    for result in results.iter_mut() {
        if let Some(node_id) = result.org_node_id {
            result.org_path = path_map.get(&node_id).cloned().or_else(|| result.org_node_name.clone());
        }
    }

    Ok(())
}

async fn hydrate_assets_by_ids(
    db: &DatabaseConnection,
    ordered_ids: &[i64],
    include_decommissioned: bool,
) -> AppResult<Vec<AssetSearchResult>> {
    if ordered_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = ordered_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let binds: Vec<sea_orm::Value> = ordered_ids.iter().map(|&id| id.into()).collect();

    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string(), format!("e.id IN ({placeholders})")];
    if !include_decommissioned {
        where_clauses.push(format!(
            "NOT ({})",
            crate::assets::taxonomy_reference::STATUS_RETIRED_SQL
        ));
    }
    let where_sql = where_clauses.join(" AND ");

    // Preserve caller order via CASE.
    let mut order_cases = String::from("ORDER BY CASE e.id");
    for (idx, id) in ordered_ids.iter().enumerate() {
        order_cases.push_str(&format!(" WHEN {id} THEN {idx}"));
    }
    order_cases.push_str(" ELSE 9999 END");

    let sql = format!("SELECT {SEARCH_SELECT} {SEARCH_FROM} WHERE {where_sql} {order_cases}");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut results: Vec<AssetSearchResult> = rows.iter().map(map_search_result).collect::<AppResult<_>>()?;
    enrich_org_paths(db, &mut results).await?;
    Ok(results)
}

async fn usage_asset_ids(db: &DatabaseConnection, user_id: Option<i64>, limit: u64) -> AppResult<Vec<i64>> {
    let (sql, binds): (String, Vec<sea_orm::Value>) = if let Some(uid) = user_id {
        (
            format!(
                "WITH usage AS (
                    SELECT equipment_id AS asset_id, created_at AS used_at
                    FROM work_orders
                    WHERE requester_id = ? AND equipment_id IS NOT NULL
                    UNION ALL
                    SELECT equipment_id AS asset_id, submitted_at AS used_at
                    FROM intervention_requests
                    WHERE submitter_id = ? AND equipment_id IS NOT NULL AND equipment_id > 0
                 )
                 SELECT asset_id
                 FROM usage
                 GROUP BY asset_id
                 ORDER BY MAX(used_at) DESC
                 LIMIT {limit}"
            ),
            vec![uid.into(), uid.into()],
        )
    } else {
        (
            format!(
                "WITH usage AS (
                    SELECT equipment_id AS asset_id, created_at AS used_at
                    FROM work_orders
                    WHERE equipment_id IS NOT NULL
                    UNION ALL
                    SELECT equipment_id AS asset_id, submitted_at AS used_at
                    FROM intervention_requests
                    WHERE equipment_id IS NOT NULL AND equipment_id > 0
                 )
                 SELECT asset_id
                 FROM usage
                 GROUP BY asset_id
                 ORDER BY COUNT(*) DESC, MAX(used_at) DESC
                 LIMIT {limit}"
            ),
            vec![],
        )
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?;
        ids.push(id);
    }
    Ok(ids)
}

// ─── Text matching ────────────────────────────────────────────────────────────

/// Ids of the live assets matching a search text, best match first.
///
/// Matching is done here with [`fold_search_text`] rather than in SQL: folding accents in SQL
/// needs dozens of nested `REPLACE()` calls per column, which overflows the parser stack of the
/// SQLCipher build ("parser stack overflow"), so every text search failed in the app.
/// Rank: exact code, then code prefix, then code / name / serial number / external id containing
/// the text; ties ordered by code.
pub(crate) async fn ranked_text_matches(db: &impl ConnectionTrait, query: &str) -> AppResult<Vec<i64>> {
    #[cfg(test)]
    TEXT_MATCH_SCANS.with(|c| c.set(c.get() + 1));
    let folded = fold_search_text(query.trim());
    if folded.is_empty() {
        return Ok(Vec::new());
    }
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT e.id, e.asset_id_code, e.name, COALESCE(e.serial_number, '') AS serial_number \
             FROM equipment e WHERE e.deleted_at IS NULL"
                .to_string(),
        ))
        .await?;
    let external = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT asset_id, external_id FROM asset_external_ids WHERE valid_to IS NULL".to_string(),
        ))
        .await?;
    let mut external_match: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for row in &external {
        let asset_id: i64 = row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?;
        let ext: String = row
            .try_get("", "external_id")
            .map_err(|e| decode_err("external_id", e))?;
        if fold_search_text(&ext).contains(&folded) {
            external_match.insert(asset_id);
        }
    }
    let mut ranked: Vec<(u8, String, i64)> = Vec::new();
    for row in &rows {
        let id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = row
            .try_get("", "asset_id_code")
            .map_err(|e| decode_err("asset_id_code", e))?;
        let name: String = row.try_get("", "name").map_err(|e| decode_err("name", e))?;
        let serial: String = row
            .try_get("", "serial_number")
            .map_err(|e| decode_err("serial_number", e))?;
        let code_f = fold_search_text(&code);
        let rank = if code_f == folded {
            0
        } else if code_f.starts_with(&folded) {
            1
        } else if code_f.contains(&folded)
            || fold_search_text(&name).contains(&folded)
            || fold_search_text(&serial).contains(&folded)
            || external_match.contains(&id)
        {
            2
        } else {
            continue;
        };
        ranked.push((rank, code, id));
    }
    ranked.sort();
    Ok(ranked.into_iter().map(|(_, _, id)| id).collect())
}

#[cfg(test)]
thread_local! {
    /// Number of [`ranked_text_matches`] scans on this thread (tests bound the scans per request).
    pub(crate) static TEXT_MATCH_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Ranked matches of the filters' search text (`None` without a text). Compute it once per
/// request and pass it to the queries that need it.
pub(crate) async fn text_matches_for(
    db: &impl ConnectionTrait,
    filters: &AssetSearchFilters,
) -> AppResult<Option<Vec<i64>>> {
    match filters.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => Ok(Some(ranked_text_matches(db, q).await?)),
        None => Ok(None),
    }
}

/// JSON array of ids, bound once and read with `json_each` (no bind-variable limit).
pub(crate) fn ids_json(ids: &[i64]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".into())
}

// ─── SQL fragments ────────────────────────────────────────────────────────────

/// Enriched SELECT for search results. Class / family / subfamily / criticality /
/// status expressions match `ASSET_SELECT` in identity.rs so List and Details share SSOT.
pub(crate) const SEARCH_SELECT: &str = concat!(
    r"
    e.id,
    e.entity_sync_id,
    e.asset_id_code         AS asset_code,
    e.name                  AS asset_name,
    COALESCE(rs_class.code, ec.code) AS class_code,
    COALESCE(rs_class.label, ec.name) AS class_name,
    rs_fam.code             AS family_code,
    rs_fam.label            AS family_name,
    rs_sub.code             AS subfamily_code,
    rs_sub.label            AS subfamily_name,
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
    ",
    crate::reference::units::unit_symbol_of!("pm.unit_value_id"),
    r" AS primary_meter_unit,
    pm.last_read_at         AS primary_meter_last_read_at,
    -- External ID count
    COALESCE(xid.ext_count, 0) AS external_id_count,
    e.row_version
"
);

pub(crate) const SEARCH_FROM: &str = r"
    FROM equipment e
    LEFT JOIN equipment_classes ec ON ec.id = e.class_id
    LEFT JOIN reference_values rs_class ON rs_class.id = e.equipment_class_ref_id
    LEFT JOIN reference_values rs_fam ON rs_fam.id = e.equipment_family_ref_id
    LEFT JOIN reference_values rs_sub ON rs_sub.id = e.equipment_subfamily_ref_id
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

/// Lightweight FROM for aggregate metrics (same filter joins as search, no hierarchy/meter enrichment).
pub(crate) const METRICS_FROM: &str = r"
    FROM equipment e
    LEFT JOIN equipment_classes ec ON ec.id = e.class_id
    LEFT JOIN reference_values rs_class ON rs_class.id = e.equipment_class_ref_id
    LEFT JOIN reference_values rs_fam ON rs_fam.id = e.equipment_family_ref_id
    LEFT JOIN lookup_values lv ON lv.id = e.criticality_value_id
    LEFT JOIN reference_values rs_crit ON rs_crit.id = e.equipment_criticality_ref_id
    LEFT JOIN reference_values rs_stat ON rs_stat.id = e.equipment_status_ref_id
";

/// Build WHERE + binds shared by search and registry metrics (SmartFilterBar scope).
/// `text_matches`: ids from [`ranked_text_matches`] when the filters carry a search text.
pub(crate) fn build_asset_scope_where(
    filters: &AssetSearchFilters,
    text_matches: Option<&[i64]>,
) -> (String, Vec<sea_orm::Value>) {
    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if !filters.include_decommissioned.unwrap_or(false) {
        where_clauses.push(format!(
            "NOT ({})",
            crate::assets::taxonomy_reference::STATUS_RETIRED_SQL
        ));
    }

    if let Some(ids) = text_matches {
        where_clauses.push("e.id IN (SELECT value FROM json_each(?))".to_string());
        binds.push(ids_json(ids).into());
    }

    if let Some(ref codes) = filters.class_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("COALESCE(rs_class.code, ec.code) IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    if let Some(ref codes) = filters.family_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("rs_fam.code IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    if let Some(ref codes) = filters.status_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!(
                "COALESCE(rs_stat.code, e.lifecycle_status) IN ({placeholders})"
            ));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    if let Some(ref ids) = filters.org_node_ids {
        if !ids.is_empty() {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            if filters.org_subtree.unwrap_or(false) {
                where_clauses.push(format!(
                    "e.installed_at_node_id IN (                       SELECT d.id FROM org_nodes d                        JOIN org_nodes a ON d.ancestor_path LIKE a.ancestor_path || '%'                        WHERE a.id IN ({placeholders}))"
                ));
            } else {
                where_clauses.push(format!("e.installed_at_node_id IN ({placeholders})"));
            }
            for &node_id in ids {
                binds.push(node_id.into());
            }
        }
    }

    if let Some(ref codes) = filters.criticality_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("COALESCE(rs_crit.code, lv.code) IN ({placeholders})"));
            for code in codes {
                binds.push(code.clone().into());
            }
        }
    }

    if let Some(ref focus) = filters.registry_focus {
        // The strip tile's own predicate (module definitions, reference-data tags), so the list
        // opened from a tile shows exactly the assets it counts.
        if let Some(sql) = crate::assets::registry_metrics::registry_focus_sql(focus) {
            where_clauses.push(sql);
        }
    }

    (where_clauses.join(" AND "), binds)
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Domain-aware asset search with multi-criteria filtering.
///
/// Search ranking: asset code exact match is ordered first (via CASE expression),
/// then code prefix, then name/serial partial match. Matching is accent- and
/// case-insensitive.
pub async fn search_assets(db: &DatabaseConnection, filters: AssetSearchFilters) -> AppResult<Vec<AssetSearchResult>> {
    let text_matches = text_matches_for(db, &filters).await?;
    search_assets_with(db, &filters, text_matches.as_deref()).await
}

/// One page of results for already computed text matches. Matches are joined once through
/// `json_each` and ordered by their position in the ranked list (rank, then code).
async fn search_assets_with(
    db: &DatabaseConnection,
    filters: &AssetSearchFilters,
    text_matches: Option<&[i64]>,
) -> AppResult<Vec<AssetSearchResult>> {
    let (where_sql, where_binds) = build_asset_scope_where(filters, None);
    let row_offset = filters.offset.unwrap_or(0);
    let row_limit = filters.limit.unwrap_or(SEARCH_PAGE_DEFAULT).min(SEARCH_PAGE_MAX);

    let mut binds: Vec<sea_orm::Value> = Vec::new();
    let (join_sql, order_sql) = match text_matches {
        Some(ids) => {
            binds.push(ids_json(ids).into());
            ("JOIN json_each(?) r ON r.value = e.id", "ORDER BY r.key")
        }
        None => ("", "ORDER BY e.asset_id_code ASC"),
    };
    binds.extend(where_binds);

    let sql = format!(
        "SELECT {SEARCH_SELECT} {SEARCH_FROM} {join_sql} WHERE {where_sql} {order_sql} \
         LIMIT {row_limit} OFFSET {row_offset}"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut results: Vec<AssetSearchResult> = rows.iter().map(map_search_result).collect::<AppResult<_>>()?;
    enrich_org_paths(db, &mut results).await?;
    Ok(results)
}

/// Paged registry list: one page of [`search_assets`] plus the total matching the same filters.
/// The text matches are computed once and shared by the count and the page.
pub async fn search_assets_page(db: &DatabaseConnection, filters: AssetSearchFilters) -> AppResult<AssetSearchPage> {
    let text_matches = text_matches_for(db, &filters).await?;
    let (where_sql, binds) = build_asset_scope_where(&filters, text_matches.as_deref());
    let count_sql = format!("SELECT COUNT(*) AS total {METRICS_FROM} WHERE {where_sql}");
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &count_sql, binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("asset count query returned no row")))?
        .try_get("", "total")
        .map_err(|e| decode_err("total", e))?;
    let items = search_assets_with(db, &filters, text_matches.as_deref()).await?;
    Ok(AssetSearchPage { items, total })
}

/// Suggestions for the shared AssetPicker.
///
/// - Non-empty query → accent-aware [`search_assets`].
/// - Empty query → recent assets for `user_id`; if none, global frequent.
pub async fn suggest_picker_assets(
    db: &DatabaseConnection,
    user_id: i64,
    filters: AssetPickerSuggestFilters,
) -> AppResult<AssetPickerSuggestions> {
    let limit = filters.limit.unwrap_or(15).min(20);
    let include_decommissioned = filters.include_decommissioned.unwrap_or(false);

    let trimmed = filters.query.as_deref().map(str::trim).filter(|q| !q.is_empty());

    if trimmed.is_some() {
        let items = search_assets(
            db,
            AssetSearchFilters {
                query: filters.query,
                class_codes: None,
                family_codes: None,
                status_codes: None,
                org_node_ids: None,
                include_decommissioned: Some(include_decommissioned),
                limit: Some(limit),
                registry_focus: None,
                offset: None,
                criticality_codes: None,
                org_subtree: None,
            },
        )
        .await?;
        return Ok(AssetPickerSuggestions {
            mode: "search".into(),
            items,
        });
    }

    let recent_ids = usage_asset_ids(db, Some(user_id), limit).await?;
    if !recent_ids.is_empty() {
        let items = hydrate_assets_by_ids(db, &recent_ids, include_decommissioned).await?;
        if !items.is_empty() {
            return Ok(AssetPickerSuggestions {
                mode: "recent".into(),
                items,
            });
        }
    }

    let frequent_ids = usage_asset_ids(db, None, limit).await?;
    let items = hydrate_assets_by_ids(db, &frequent_ids, include_decommissioned).await?;
    Ok(AssetPickerSuggestions {
        mode: "frequent".into(),
        items,
    })
}

/// Suggest asset codes matching a prefix. Returns lightweight entries for
/// typeahead / autocomplete controls.
pub async fn suggest_asset_codes(
    db: &DatabaseConnection,
    prefix: &str,
    limit: Option<u64>,
) -> AppResult<Vec<AssetSuggestion>> {
    let folded = fold_search_text(prefix.trim());
    if folded.is_empty() {
        return Ok(Vec::new());
    }
    let row_limit = usize::try_from(limit.unwrap_or(10).min(50)).unwrap_or(10);
    let rows = suggestion_rows(db).await?;
    let mut out: Vec<AssetSuggestion> = rows
        .into_iter()
        .filter(|s| fold_search_text(&s.asset_code).starts_with(&folded))
        .collect();
    out.sort_by(|a, b| a.asset_code.cmp(&b.asset_code));
    out.truncate(row_limit);
    Ok(out)
}

/// Every live asset as a suggestion row (code, name, status); matching happens in Rust.
async fn suggestion_rows(db: &DatabaseConnection) -> AppResult<Vec<AssetSuggestion>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT e.id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                    e.lifecycle_status AS status_code \
             FROM equipment e WHERE e.deleted_at IS NULL"
                .to_string(),
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
    let folded = fold_search_text(partial.trim());
    if folded.is_empty() {
        return Ok(Vec::new());
    }
    let row_limit = usize::try_from(limit.unwrap_or(10).min(50)).unwrap_or(10);
    let rows = suggestion_rows(db).await?;
    let mut out: Vec<AssetSuggestion> = rows
        .into_iter()
        .filter(|s| fold_search_text(&s.asset_name).contains(&folded))
        .collect();
    out.sort_by(|a, b| a.asset_name.cmp(&b.asset_name));
    out.truncate(row_limit);
    Ok(out)
}

#[cfg(test)]
mod fold_tests {
    use super::fold_search_text;

    #[test]
    fn folds_french_accents_and_case() {
        assert_eq!(fold_search_text("Compresseur"), "compresseur");
        assert_eq!(fold_search_text("COMPRESSEUR"), "compresseur");
        assert_eq!(fold_search_text("Compresseùr"), "compresseur");
        assert_eq!(fold_search_text("Électricité"), "electricite");
        assert_eq!(fold_search_text("çà"), "ca");
    }
}

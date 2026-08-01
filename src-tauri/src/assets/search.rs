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
}

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
    pub sync_id: String,
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

/// SQLite expression that folds a text column the same way as [`fold_search_text`].
fn sql_fold_expr(column_sql: &str) -> String {
    // Nest REPLACE for diacritics then LOWER for ASCII case.
    // Order: map accented chars → base, then LOWER.
    let pairs = [
        ("à", "a"),
        ("á", "a"),
        ("â", "a"),
        ("ä", "a"),
        ("ã", "a"),
        ("å", "a"),
        ("À", "a"),
        ("Á", "a"),
        ("Â", "a"),
        ("Ä", "a"),
        ("è", "e"),
        ("é", "e"),
        ("ê", "e"),
        ("ë", "e"),
        ("È", "e"),
        ("É", "e"),
        ("Ê", "e"),
        ("Ë", "e"),
        ("ì", "i"),
        ("í", "i"),
        ("î", "i"),
        ("ï", "i"),
        ("Ì", "i"),
        ("Í", "i"),
        ("Î", "i"),
        ("Ï", "i"),
        ("ò", "o"),
        ("ó", "o"),
        ("ô", "o"),
        ("ö", "o"),
        ("õ", "o"),
        ("Ò", "o"),
        ("Ó", "o"),
        ("Ô", "o"),
        ("Ö", "o"),
        ("ù", "u"),
        ("ú", "u"),
        ("û", "u"),
        ("ü", "u"),
        ("Ù", "u"),
        ("Ú", "u"),
        ("Û", "u"),
        ("Ü", "u"),
        ("ý", "y"),
        ("ÿ", "y"),
        ("ç", "c"),
        ("Ç", "c"),
        ("ñ", "n"),
        ("Ñ", "n"),
    ];
    let mut expr = column_sql.to_string();
    for (from, to) in pairs {
        expr = format!("REPLACE({expr}, '{from}', '{to}')");
    }
    // Multi-char ligatures
    expr = format!("REPLACE({expr}, 'œ', 'oe')");
    expr = format!("REPLACE({expr}, 'Œ', 'oe')");
    expr = format!("REPLACE({expr}, 'æ', 'ae')");
    expr = format!("REPLACE({expr}, 'Æ', 'ae')");
    format!("LOWER({expr})")
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

/// Resolve org paths for the returned result set only (bounded by result count).
async fn enrich_org_paths(
    db: &DatabaseConnection,
    results: &mut [AssetSearchResult],
) -> AppResult<()> {
    let mut node_ids: Vec<i64> = results
        .iter()
        .filter_map(|r| r.org_node_id)
        .collect();
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
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    let mut path_parts: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        let leaf_id: i64 = row
            .try_get("", "leaf_id")
            .map_err(|e| decode_err("leaf_id", e))?;
        let name: String = row
            .try_get("", "name")
            .map_err(|e| decode_err("name", e))?;
        path_parts.entry(leaf_id).or_default().push(name);
    }

    let path_map: HashMap<i64, String> = path_parts
        .into_iter()
        .map(|(id, parts)| (id, parts.join(" / ")))
        .collect();

    for result in results.iter_mut() {
        if let Some(node_id) = result.org_node_id {
            result.org_path = path_map.get(&node_id).cloned().or_else(|| {
                result.org_node_name.clone()
            });
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

    let mut where_clauses = vec![
        "e.deleted_at IS NULL".to_string(),
        format!("e.id IN ({placeholders})"),
    ];
    if !include_decommissioned {
        where_clauses.push(
            "COALESCE(rs_stat.code, e.lifecycle_status) NOT IN ('DECOMMISSIONED', 'SCRAPPED')"
                .to_string(),
        );
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
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    let mut results: Vec<AssetSearchResult> = rows.iter().map(map_search_result).collect::<AppResult<_>>()?;
    enrich_org_paths(db, &mut results).await?;
    Ok(results)
}

async fn usage_asset_ids(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    limit: u64,
) -> AppResult<Vec<i64>> {
    let (sql, binds): (String, Vec<sea_orm::Value>) = if let Some(uid) = user_id {
        (
            format!(
                "WITH usage AS (
                    SELECT equipment_id AS asset_id, created_at AS used_at
                    FROM work_orders
                    WHERE requester_id = ? AND equipment_id IS NOT NULL
                    UNION ALL
                    SELECT asset_id, submitted_at AS used_at
                    FROM intervention_requests
                    WHERE submitter_id = ? AND asset_id IS NOT NULL AND asset_id > 0
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
                    SELECT asset_id, submitted_at AS used_at
                    FROM intervention_requests
                    WHERE asset_id IS NOT NULL AND asset_id > 0
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
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            binds,
        ))
        .await?;

    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row
            .try_get("", "asset_id")
            .map_err(|e| decode_err("asset_id", e))?;
        ids.push(id);
    }
    Ok(ids)
}

// ─── SQL fragments ────────────────────────────────────────────────────────────

/// Enriched SELECT for search results. Class / family / subfamily / criticality /
/// status expressions match `ASSET_SELECT` in identity.rs so List and Details share SSOT.
const SEARCH_SELECT: &str = r"
    e.id,
    e.sync_id,
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
    pm.unit                 AS primary_meter_unit,
    pm.last_read_at         AS primary_meter_last_read_at,
    -- External ID count
    COALESCE(xid.ext_count, 0) AS external_id_count,
    e.row_version
";

const SEARCH_FROM: &str = r"
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

// ─── Service functions ────────────────────────────────────────────────────────

/// Domain-aware asset search with multi-criteria filtering.
///
/// Search ranking: asset code exact match is ordered first (via CASE expression),
/// then code prefix, then name/serial partial match. Matching is accent- and
/// case-insensitive.
pub async fn search_assets(
    db: &DatabaseConnection,
    filters: AssetSearchFilters,
) -> AppResult<Vec<AssetSearchResult>> {
    let mut where_clauses = vec!["e.deleted_at IS NULL".to_string()];
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if !filters.include_decommissioned.unwrap_or(false) {
        where_clauses.push(
            "COALESCE(rs_stat.code, e.lifecycle_status) NOT IN ('DECOMMISSIONED', 'SCRAPPED')"
                .to_string(),
        );
    }

    let folded_query = filters
        .query
        .as_ref()
        .map(|q| fold_search_text(q.trim()))
        .filter(|q| !q.is_empty());

    if let Some(ref folded) = folded_query {
        let code_fold = sql_fold_expr("e.asset_id_code");
        let name_fold = sql_fold_expr("e.name");
        let serial_fold = sql_fold_expr("COALESCE(e.serial_number, '')");
        let ext_fold = sql_fold_expr("external_id");
        where_clauses.push(format!(
            "({code_fold} LIKE ? OR {name_fold} LIKE ? OR {serial_fold} LIKE ? \
             OR e.id IN (SELECT asset_id FROM asset_external_ids \
               WHERE {ext_fold} LIKE ? AND valid_to IS NULL))"
        ));
        let pattern = format!("%{folded}%");
        binds.push(pattern.clone().into());
        binds.push(pattern.clone().into());
        binds.push(pattern.clone().into());
        binds.push(pattern.into());
    }

    if let Some(ref codes) = filters.class_codes {
        if !codes.is_empty() {
            let placeholders = codes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!(
                "COALESCE(rs_class.code, ec.code) IN ({placeholders})"
            ));
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
            where_clauses.push(format!("e.installed_at_node_id IN ({placeholders})"));
            for &node_id in ids {
                binds.push(node_id.into());
            }
        }
    }

    let where_sql = where_clauses.join(" AND ");
    let row_limit = filters.limit.unwrap_or(100).min(200);

    let order_sql = if let Some(ref folded) = folded_query {
        let code_fold = sql_fold_expr("e.asset_id_code");
        binds.push(folded.clone().into());
        binds.push(format!("{folded}%").into());
        format!(
            "ORDER BY \
                CASE WHEN {code_fold} = ? THEN 0 \
                     WHEN {code_fold} LIKE ? THEN 1 \
                     ELSE 2 END, \
                e.asset_id_code ASC"
        )
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

    let mut results: Vec<AssetSearchResult> =
        rows.iter().map(map_search_result).collect::<AppResult<_>>()?;
    enrich_org_paths(db, &mut results).await?;
    Ok(results)
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

    let trimmed = filters
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty());

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
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let folded = fold_search_text(trimmed);
    let pattern = format!("{folded}%");
    let row_limit = limit.unwrap_or(10).min(50);
    let code_fold = sql_fold_expr("e.asset_id_code");

    let sql = format!(
        "SELECT e.id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                e.lifecycle_status AS status_code \
         FROM equipment e \
         WHERE e.deleted_at IS NULL \
           AND {code_fold} LIKE ? \
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

    let folded = fold_search_text(trimmed);
    let pattern = format!("%{folded}%");
    let row_limit = limit.unwrap_or(10).min(50);
    let name_fold = sql_fold_expr("e.name");

    let sql = format!(
        "SELECT e.id, e.asset_id_code AS asset_code, e.name AS asset_name, \
                e.lifecycle_status AS status_code \
         FROM equipment e \
         WHERE e.deleted_at IS NULL \
           AND {name_fold} LIKE ? \
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

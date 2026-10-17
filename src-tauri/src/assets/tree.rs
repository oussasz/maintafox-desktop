//! Equipment tree read from the database, one level at a time.
//!
//! The registry used to nest the assets of the loaded list page only, so on a large plant a
//! child whose parent was on another page became a root and branches went missing. This module
//! reads the hierarchy itself (`equipment_hierarchy`, active links):
//!
//! - **Visible assets** = the assets matching the registry filters (same scope as the list,
//!   `search::build_asset_scope_where`) **plus all their ancestors**, so a match deep in a
//!   branch is reachable from its root. Ancestors that do not match are returned with
//!   `is_match = false` (shown as context).
//! - **One level**: the roots (no active parent among the visible assets) or the children of one
//!   asset, paged (`limit` ≤ [`search::SEARCH_PAGE_MAX`], `offset`), with the number of visible
//!   children of each row so the screen knows what can be expanded.
//!
//! Same rows and permission as the registry list (`search_assets_page`).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::assets::search::{
    build_asset_scope_where, enrich_org_paths, map_search_result, text_matches_for, AssetSearchFilters,
    AssetSearchResult, METRICS_FROM, SEARCH_FROM, SEARCH_PAGE_DEFAULT, SEARCH_PAGE_MAX, SEARCH_SELECT,
};
use crate::errors::{AppError, AppResult};

/// One asset of a tree level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTreeRow {
    #[serde(flatten)]
    pub asset: AssetSearchResult,
    /// Visible children (matching, or leading to a match).
    pub child_count: i64,
    /// The asset itself matches the filters (false: shown as the parent of a match).
    pub is_match: bool,
}

/// One page of one level of the tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetTreeLevel {
    pub parent_id: Option<i64>,
    pub items: Vec<AssetTreeRow>,
    /// Rows of this level (all pages).
    pub total: i64,
}

/// `matched` (assets in the filter scope) and `visible` (matched + active ancestors, not
/// deleted). `UNION` keeps the recursion finite even on a cyclic hierarchy.
fn scope_ctes(where_sql: &str) -> String {
    format!(
        "WITH RECURSIVE matched(id) AS (SELECT e.id {METRICS_FROM} WHERE {where_sql}), \
         visible(id) AS ( \
            SELECT id FROM matched \
            UNION \
            SELECT h.parent_equipment_id FROM equipment_hierarchy h \
              JOIN visible v ON h.child_equipment_id = v.id \
              JOIN equipment p ON p.id = h.parent_equipment_id AND p.deleted_at IS NULL \
             WHERE h.effective_to IS NULL)"
    )
}

/// Rows of the level: the roots, or the children of `parent_id`.
fn level_sql(parent_id: Option<i64>) -> &'static str {
    match parent_id {
        None => "(ph.parent_equipment_id IS NULL OR ph.parent_equipment_id NOT IN (SELECT id FROM visible))",
        Some(_) => "ph.parent_equipment_id = ?",
    }
}

pub async fn list_asset_tree_level(
    db: &DatabaseConnection,
    filters: AssetSearchFilters,
    parent_id: Option<i64>,
) -> AppResult<AssetTreeLevel> {
    let text_matches = text_matches_for(db, &filters).await?;
    let (where_sql, scope_binds) = build_asset_scope_where(&filters, text_matches.as_deref());
    let ctes = scope_ctes(&where_sql);
    let level = level_sql(parent_id);
    let level_binds = || -> Vec<sea_orm::Value> {
        let mut binds = scope_binds.clone();
        if let Some(pid) = parent_id {
            binds.push(pid.into());
        }
        binds
    };

    let count_sql = format!(
        "{ctes} SELECT COUNT(*) AS total {SEARCH_FROM} \
         WHERE e.deleted_at IS NULL AND e.id IN (SELECT id FROM visible) AND {level}"
    );
    let total: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &count_sql,
            level_binds(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("asset tree count returned no row")))?
        .try_get("", "total")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("asset tree total decode: {e}")))?;

    let limit = filters.limit.unwrap_or(SEARCH_PAGE_DEFAULT).clamp(1, SEARCH_PAGE_MAX);
    let offset = filters.offset.unwrap_or(0);
    let sql = format!(
        "{ctes} SELECT {SEARCH_SELECT}, \
            (SELECT COUNT(*) FROM equipment_hierarchy c \
              WHERE c.parent_equipment_id = e.id AND c.effective_to IS NULL \
                AND c.child_equipment_id IN (SELECT id FROM visible)) AS child_count, \
            CASE WHEN e.id IN (SELECT id FROM matched) THEN 1 ELSE 0 END AS is_match \
         {SEARCH_FROM} \
         WHERE e.deleted_at IS NULL AND e.id IN (SELECT id FROM visible) AND {level} \
         ORDER BY e.asset_id_code ASC LIMIT {limit} OFFSET {offset}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, level_binds()))
        .await?;

    let mut assets: Vec<AssetSearchResult> = rows.iter().map(map_search_result).collect::<AppResult<_>>()?;
    enrich_org_paths(db, &mut assets).await?;
    let items = rows
        .iter()
        .zip(assets)
        .map(|(row, asset)| {
            let decode = |e: sea_orm::DbErr| AppError::Internal(anyhow::anyhow!("asset tree row decode: {e}"));
            Ok(AssetTreeRow {
                asset,
                child_count: row.try_get("", "child_count").map_err(decode)?,
                is_match: row.try_get::<i64>("", "is_match").map_err(decode)? != 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(AssetTreeLevel {
        parent_id,
        items,
        total,
    })
}

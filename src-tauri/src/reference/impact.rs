//! Publish impact of a reference set (Phase 6.8): which records use a value that the candidate
//! set removes or deactivates.
//!
//! Records keep pointing at the values of the set that was published when they were saved, and
//! several modules still use the operational copy of a domain in `lookup_values` (same codes).
//! The changed codes are therefore the codes of the published set that the candidate drops or
//! deactivates (plus the candidate's own inactive codes), and their usage is counted through
//! every foreign key of the schema that points to `reference_values` or `lookup_values` — found
//! with `pragma_foreign_key_list`, so a table added later is counted without changing this file.
//! Catalog tables themselves (`reference_*`, `lookup_*`) are not usage.

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// Module dimensions of the impact preview, in display order.
pub const DIMENSIONS: &[&str] = &[
    "assets",
    "work_orders",
    "pm_plans",
    "inventory",
    "reliability_events",
    "interventions",
    "personnel",
    "other",
];

/// Values of a domain that publishing the candidate set changes.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ValueChanges {
    /// Codes of the published set missing from the candidate.
    pub removed: Vec<String>,
    /// Codes active in the published set (or new in the candidate) and inactive in the candidate.
    pub deactivated: Vec<String>,
}

impl ValueChanges {
    pub fn codes(&self) -> BTreeSet<String> {
        self.removed.iter().chain(&self.deactivated).cloned().collect()
    }
}

/// Usage of the changed values in one column.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ColumnUsage {
    pub table: String,
    pub column: String,
    /// `reference_values` or `lookup_values`.
    pub target: String,
    pub count: i64,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("reference impact decode '{field}': {err}"))
}

/// Module of a table that references catalog values.
pub fn module_of(table: &str) -> &'static str {
    let t = table.to_ascii_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| t.starts_with(p));
    if starts(&["work_order", "wo_", "work_delay"]) {
        "work_orders"
    } else if starts(&["pm_"]) {
        "pm_plans"
    } else if starts(&[
        "article",
        "inventory",
        "stock",
        "warehouse",
        "purchase",
        "supplier",
        "procurement",
        "repairable",
        "unit_conversion",
        "goods_",
        "requisition",
    ]) {
        "inventory"
    } else if starts(&[
        "failure_",
        "fmeca",
        "rcm_",
        "reliability",
        "weibull",
        "ram_",
        "fta_",
        "rbd_",
    ]) {
        "reliability_events"
    } else if starts(&["equipment", "asset", "component", "meter"]) {
        "assets"
    } else if starts(&["intervention_request", "di_"]) {
        "interventions"
    } else if starts(&[
        "personnel",
        "crew",
        "skill",
        "team",
        "training",
        "qualification",
        "user_",
        "schedule",
    ]) {
        "personnel"
    } else {
        "other"
    }
}

async fn set_codes(conn: &impl ConnectionTrait, set_id: i64) -> AppResult<BTreeMap<String, bool>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code, is_active FROM reference_values WHERE set_id = ?",
            [set_id.into()],
        ))
        .await?;
    let mut out = BTreeMap::new();
    for r in &rows {
        let code: String = r.try_get("", "code").map_err(|e| decode_err("code", e))?;
        let active: i64 = r.try_get("", "is_active").map_err(|e| decode_err("is_active", e))?;
        out.insert(code, active != 0);
    }
    Ok(out)
}

/// Values the candidate set changes against the published set of the same domain.
pub async fn value_changes(
    conn: &impl ConnectionTrait,
    published_set_id: Option<i64>,
    candidate_set_id: i64,
) -> AppResult<ValueChanges> {
    let candidate = set_codes(conn, candidate_set_id).await?;
    let published = match published_set_id {
        Some(id) => set_codes(conn, id).await?,
        None => BTreeMap::new(),
    };
    let mut changes = ValueChanges::default();
    for (code, active) in &published {
        match candidate.get(code) {
            None => changes.removed.push(code.clone()),
            Some(false) if *active => changes.deactivated.push(code.clone()),
            _ => {}
        }
    }
    for (code, active) in &candidate {
        if !active && !published.contains_key(code) {
            changes.deactivated.push(code.clone());
        }
    }
    changes.deactivated.sort();
    Ok(changes)
}

/// `(table, column)` of every foreign key to `target(id)`, catalog tables excluded.
pub async fn referencing_columns(conn: &impl ConnectionTrait, target: &str) -> AppResult<Vec<(String, String)>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT m.name AS tbl, p.\"from\" AS col FROM sqlite_master m \
             JOIN pragma_foreign_key_list(m.name) p \
             WHERE m.type = 'table' AND p.\"table\" = ? \
               AND substr(m.name, 1, 10) <> 'reference_' AND substr(m.name, 1, 7) <> 'lookup_' \
               AND substr(m.name, 1, 7) <> 'sqlite_' \
             ORDER BY m.name, p.\"from\"",
            [target.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                r.try_get("", "tbl").map_err(|e| decode_err("tbl", e))?,
                r.try_get("", "col").map_err(|e| decode_err("col", e))?,
            ))
        })
        .collect()
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Rows of each referencing column that point to one of `ids` (`target` = catalog table).
pub async fn count_usage(conn: &impl ConnectionTrait, target: &str, ids: &[i64]) -> AppResult<Vec<ColumnUsage>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let values: Vec<Value> = ids.iter().map(|id| (*id).into()).collect();
    let mut out = Vec::new();
    for (table, column) in referencing_columns(conn, target).await? {
        let row = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS n FROM {} WHERE {} IN ({placeholders})",
                    quote_ident(&table),
                    quote_ident(&column)
                ),
                values.clone(),
            ))
            .await?;
        let count: i64 = row
            .map(|r| r.try_get("", "n"))
            .transpose()
            .map_err(|e| decode_err("n", e))?
            .unwrap_or(0);
        if count > 0 {
            out.push(ColumnUsage {
                table,
                column,
                target: target.to_string(),
                count,
            });
        }
    }
    Ok(out)
}

/// Ids selected by `sql`, whose `{codes}` placeholder is replaced by one `?` per code.
async fn ids_for_codes(
    conn: &impl ConnectionTrait,
    sql: &str,
    mut values: Vec<Value>,
    codes: &BTreeSet<String>,
) -> AppResult<Vec<i64>> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; codes.len()].join(", ");
    values.extend(codes.iter().map(|c| Value::from(c.clone())));
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql.replace("{codes}", &placeholders),
            values,
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
        .collect()
}

/// Usage of the changed codes: values of the published set (records saved against it) and of
/// the candidate, plus the operational copy of the domain in `lookup_values`.
pub async fn usage_of_changes(
    conn: &impl ConnectionTrait,
    domain_code: &str,
    published_set_id: Option<i64>,
    candidate_set_id: i64,
    codes: &BTreeSet<String>,
) -> AppResult<Vec<ColumnUsage>> {
    let by_set = "SELECT id FROM reference_values WHERE set_id = ? AND code IN ({codes})";
    let mut reference_ids = ids_for_codes(conn, by_set, vec![candidate_set_id.into()], codes).await?;
    if let Some(published) = published_set_id {
        reference_ids.extend(ids_for_codes(conn, by_set, vec![published.into()], codes).await?);
    }
    // The lookup copy of the domain: same key, or the key with `_` read as `.` (the convention
    // of `reference::migrations::remap_lookup_consumer_usage`).
    let lookup_ids = ids_for_codes(
        conn,
        "SELECT lv.id AS id FROM lookup_values lv JOIN lookup_domains ld ON ld.id = lv.domain_id \
         WHERE ld.deleted_at IS NULL AND lv.deleted_at IS NULL \
           AND (LOWER(ld.domain_key) = LOWER(?) OR LOWER(ld.domain_key) = LOWER(REPLACE(?, '_', '.'))) \
           AND lv.code IN ({codes})",
        vec![domain_code.into(), domain_code.into()],
        codes,
    )
    .await?;
    let mut usage = count_usage(conn, "reference_values", &reference_ids).await?;
    usage.extend(count_usage(conn, "lookup_values", &lookup_ids).await?);
    Ok(usage)
}

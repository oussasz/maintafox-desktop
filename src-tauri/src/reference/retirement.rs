//! Retiring a reference list that another list replaces (data migrations only).
//!
//! When two lists are merged (for example the request symptoms `DI.SYMPTOM` into the shared
//! ISO 14224 symptom list `WORK.SYMPTOMS`), every stored link to a retired value is moved to its
//! replacement, then the retired values, sets and domain are deleted. The links are found from
//! the schema itself: every declared foreign key on `reference_values(id)`, plus the references
//! the sync row specifications declare without a foreign key. Rows of the reference engine's own
//! bookkeeping tables (`reference_*`: aliases, migration maps, validation reports…) that still
//! point at a retired row are deleted with it. A business row that still points at a retired value
//! after the remap stops the migration with the table and column named, so no link is ever lost
//! silently.

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `(table, column)` pairs holding ids of `target_table`: declared foreign keys plus the
/// references the sync row specifications declare.
pub async fn referencing_columns(conn: &impl ConnectionTrait, target_table: &str) -> AppResult<Vec<(String, String)>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT m.name AS tbl, f.\"from\" AS col \
               FROM sqlite_master m \
               JOIN pragma_foreign_key_list(m.name) f \
              WHERE m.type = 'table' AND f.\"table\" = ?",
            [target_table.into()],
        ))
        .await?;
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for r in rows {
        let tbl: String = r
            .try_get("", "tbl")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("fk table: {e}")))?;
        let col: String = r
            .try_get("", "col")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("fk column: {e}")))?;
        out.insert((tbl, col));
    }
    for spec in crate::sync::row_spec::ROW_SPECS {
        for r in spec.refs {
            if let crate::sync::row_spec::RefTarget::Entity(entity) = r.target {
                if entity == target_table && table_has_column(conn, spec.entity_type, r.column).await? {
                    out.insert((spec.entity_type.to_string(), r.column.to_string()));
                }
            }
        }
    }
    Ok(out.into_iter().collect())
}

async fn table_has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

fn is_reference_bookkeeping(table: &str) -> bool {
    table.starts_with("reference_")
}

/// Moves every link from `old_id` to `new_id` (both `reference_values` ids).
pub async fn remap_value_links(conn: &impl ConnectionTrait, pairs: &[(i64, i64)]) -> AppResult<()> {
    if pairs.is_empty() {
        return Ok(());
    }
    let columns = referencing_columns(conn, "reference_values").await?;
    for (table, column) in &columns {
        // The value tree itself is handled by the caller (children of a retired value).
        if table == "reference_values" {
            continue;
        }
        for (old_id, new_id) in pairs {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "UPDATE OR IGNORE {t} SET {c} = ? WHERE {c} = ?",
                    t = quote(table),
                    c = quote(column)
                ),
                [(*new_id).into(), (*old_id).into()],
            ))
            .await?;
        }
    }
    Ok(())
}

async fn delete_rows_pointing_at(
    conn: &impl ConnectionTrait,
    target_table: &str,
    ids: &[i64],
    skip_table: Option<&str>,
) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    for (table, column) in referencing_columns(conn, target_table).await? {
        if Some(table.as_str()) == skip_table {
            continue;
        }
        let values: Vec<sea_orm::Value> = ids.iter().map(|id| (*id).into()).collect();
        if is_reference_bookkeeping(&table) {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "DELETE FROM {t} WHERE {c} IN ({placeholders})",
                    t = quote(&table),
                    c = quote(&column)
                ),
                values,
            ))
            .await?;
            continue;
        }
        let left = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS n FROM {t} WHERE {c} IN ({placeholders})",
                    t = quote(&table),
                    c = quote(&column)
                ),
                values,
            ))
            .await?
            .and_then(|r| r.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
        if left > 0 {
            return Err(AppError::Internal(anyhow::anyhow!(
                "{left} row(s) of {table}.{column} still point at a retired {target_table} row; \
                 the list cannot be retired without losing these links"
            )));
        }
    }
    Ok(())
}

/// Deletes the domain `domain_code`, its sets and its values. Every link to its values must have
/// been moved first ([`remap_value_links`]); a remaining business link stops the deletion.
pub async fn delete_domain(conn: &impl ConnectionTrait, domain_code: &str) -> AppResult<()> {
    let domain = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = ?",
            [domain_code.trim().to_ascii_uppercase().into()],
        ))
        .await?;
    let Some(domain) = domain else {
        return Ok(());
    };
    let domain_id: i64 = domain
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("domain id: {e}")))?;

    let ids_of = |sql: &'static str| async move {
        let rows = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                [domain_id.into()],
            ))
            .await?;
        rows.iter()
            .map(|r| {
                r.try_get::<i64>("", "id")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("id: {e}")))
            })
            .collect::<AppResult<Vec<i64>>>()
    };
    let value_ids = ids_of(
        "SELECT rv.id FROM reference_values rv JOIN reference_sets rs ON rs.id = rv.set_id WHERE rs.domain_id = ?",
    )
    .await?;
    let set_ids = ids_of("SELECT id FROM reference_sets WHERE domain_id = ?").await?;

    delete_rows_pointing_at(conn, "reference_values", &value_ids, Some("reference_values")).await?;
    if !value_ids.is_empty() {
        let placeholders = vec!["?"; value_ids.len()].join(", ");
        let values = || value_ids.iter().map(|id| sea_orm::Value::from(*id)).collect::<Vec<_>>();
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("UPDATE reference_values SET parent_id = NULL WHERE parent_id IN ({placeholders})"),
            values(),
        ))
        .await?;
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("DELETE FROM reference_values WHERE id IN ({placeholders})"),
            values(),
        ))
        .await?;
    }
    delete_rows_pointing_at(conn, "reference_sets", &set_ids, None).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_sets WHERE domain_id = ?",
        [domain_id.into()],
    ))
    .await?;
    delete_rows_pointing_at(conn, "reference_domains", &[domain_id], None).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reference_domains WHERE id = ?",
        [domain_id.into()],
    ))
    .await?;
    Ok(())
}

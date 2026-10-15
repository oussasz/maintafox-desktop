//! Retiring a reference list that another list replaces (data migrations and the startup
//! retirement pass).
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

async fn domain_exists(conn: &impl ConnectionTrait, code: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_domains WHERE UPPER(TRIM(code)) = ?",
            [code.trim().to_ascii_uppercase().into()],
        ))
        .await?
        .is_some())
}

/// Merges the list `retired` into `target`: a retired value whose code exists in `target` has
/// its links moved to that value; a value only the retired list has (a plant's own value) is
/// moved, with its id, identity and links, into the latest published set of `target`. The
/// retired domain is then deleted. Returns `false` when `retired` no longer exists (nothing to do),
/// so the merge can run again safely.
pub async fn merge_domain_into(conn: &impl ConnectionTrait, retired: &str, target: &str) -> AppResult<bool> {
    if !domain_exists(conn, retired).await? {
        return Ok(false);
    }
    let target_set: i64 = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id               WHERE UPPER(TRIM(d.code)) = ? AND rs.status = 'published' ORDER BY rs.version_no DESC LIMIT 1",
            [target.to_ascii_uppercase().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("{target} has no published set")))?
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("target set id: {e}")))?;

    // Published set first, so a plant value present in several versions keeps its published row.
    let values = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code FROM reference_values rv                JOIN reference_sets rs ON rs.id = rv.set_id                JOIN reference_domains d ON d.id = rs.domain_id               WHERE UPPER(TRIM(d.code)) = ?               ORDER BY CASE rs.status WHEN 'published' THEN 0 ELSE 1 END, rs.version_no DESC, rv.id",
            [retired.to_ascii_uppercase().into()],
        ))
        .await?;
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    for v in values {
        let id: i64 = v
            .try_get("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("value id: {e}")))?;
        let code: String = v
            .try_get("", "code")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("value code: {e}")))?;
        let same_code: Option<i64> = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv                    JOIN reference_sets rs ON rs.id = rv.set_id                    JOIN reference_domains d ON d.id = rs.domain_id                   WHERE UPPER(TRIM(d.code)) = ? AND LOWER(TRIM(rv.code)) = LOWER(TRIM(?))                   ORDER BY CASE rs.status WHEN 'published' THEN 0 ELSE 1 END, rs.version_no DESC LIMIT 1",
                [target.to_ascii_uppercase().into(), code.into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id"))
            .transpose()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("target value id: {e}")))?;
        match same_code {
            Some(target_id) => pairs.push((id, target_id)),
            None => {
                conn.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE reference_values SET set_id = ?, parent_id = NULL WHERE id = ?",
                    [target_set.into(), id.into()],
                ))
                .await?;
            }
        }
    }
    remap_value_links(conn, &pairs).await?;
    delete_domain(conn, retired).await?;
    Ok(true)
}

/// Lists replaced by a shared list: `(retired domain, list that replaces it)`. `None` = the
/// retired list is not linked by id (its codes were rewritten in place), it is only deleted.
/// - `DI.PRIORITY` → shared urgency scale `WORK.URGENCY` (codes rewritten by migration 198);
/// - `DI.SYMPTOM` → shared failure-symptom list `WORK.SYMPTOMS` (migration 199).
pub const RETIRED_DOMAINS: &[(&str, Option<&str>)] = &[("DI.PRIORITY", None), ("DI.SYMPTOM", Some("WORK.SYMPTOMS"))];

/// Startup retirement pass, run once change capture is installed. Migrations run with capture
/// suspended, so their deletes and remaps are not sent; a device that replays the server history
/// could bring a retired list (and rows pointing at it) back. This pass merges or deletes any
/// retired list found again, with capture on, so the change reaches the other devices. Does
/// nothing when no retired list exists. Returns the domains it retired. Runs in one transaction:
/// a link that cannot be moved leaves everything as it was.
pub async fn retire_superseded_domains(db: &sea_orm::DatabaseConnection) -> AppResult<Vec<String>> {
    use sea_orm::TransactionTrait;
    let txn = db.begin().await?;
    let retired = retire_superseded_domains_in(&txn).await?;
    txn.commit().await?;
    Ok(retired)
}

async fn retire_superseded_domains_in(conn: &impl ConnectionTrait) -> AppResult<Vec<String>> {
    let mut retired = Vec::new();
    for (domain, target) in RETIRED_DOMAINS {
        let done = match target {
            Some(target) => merge_domain_into(conn, domain, target).await?,
            None => {
                if domain_exists(conn, domain).await? {
                    delete_domain(conn, domain).await?;
                    true
                } else {
                    false
                }
            }
        };
        if done {
            retired.push((*domain).to_string());
        }
    }
    Ok(retired)
}

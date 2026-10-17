//! Migration 178 — WO close-out failure coding on governed reference values.
//!
//! `work_order_failure_details.failure_mode_id / failure_cause_id / failure_effect_id` still
//! declared `REFERENCES failure_codes(id)` (migration 081) although failure modes moved to
//! `reference_values` (migration 114) and the close-out UI codes causes and effects from
//! reference domains: every save with a mode/cause/effect failed with
//! `FOREIGN KEY constraint failed`.
//!
//! - Seeds the close-out domains (`WORK.SYMPTOMS`, `WORK.FAILURE_CAUSES`, `WORK.FAILURE_EFFECTS`,
//!   `WORK.FAILURE_MECHANISMS`, `WORK.DETECTION_METHODS`) with ISO 14224 starter values
//!   (`wo::closeout_reference_catalog`, also run at every startup).
//! - Re-points legacy `failure_codes` ids still stored in the three columns to the matching
//!   reference value (same code, created in the right domain when missing); ids that resolve to
//!   nothing become NULL (they could not be displayed or saved anyway).
//! - Rebuilds the table (SQLite cannot alter a foreign key) with the same columns, rows, ids,
//!   indexes and triggers, the three foreign keys now on `reference_values(id)`. Views that read
//!   the table are dropped and recreated around the rebuild.
//! - Adds `failure_mechanism_id` and `detection_method_id` (ISO 14224 B.2 / B.4), both NULL by
//!   default and referencing `reference_values(id)`.
//!
//! - `failure_events` (RAMS, filled from the WO coding at close-out) had the same wrong foreign
//!   keys on mode / cause / effect / mechanism: rebuilt the same way (`failure_class_id` keeps
//!   `failure_codes`).
//!
//! `work_order_failure_details` and `failure_events` are tenant-synchronised: the column meanings are the ones the
//! application already used (reference value ids); the two new columns are additive.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

use crate::wo::closeout_reference_catalog::{
    ensure_closeout_reference_catalogs, ensure_value, DOMAIN_FAILURE_CAUSES, DOMAIN_FAILURE_EFFECTS,
    DOMAIN_FAILURE_MECHANISMS,
};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000178_wo_failure_coding_reference"
    }
}

const TABLE: &str = "work_order_failure_details";
const FAILURE_EVENTS: &str = "failure_events";
const MODES_DOMAIN: &str = "WORK.FAILURE_MODES";

fn db_err(e: impl std::fmt::Display) -> DbErr {
    DbErr::Custom(e.to_string())
}

async fn reference_value_in_domain<C: ConnectionTrait>(conn: &C, id: i64, domain: &str) -> Result<bool, DbErr> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE rv.id = ? AND UPPER(TRIM(d.code)) = ?",
            [id.into(), domain.into()],
        ))
        .await?;
    Ok(row.is_some())
}

async fn reference_value_id_by_code<C: ConnectionTrait>(
    conn: &C,
    domain: &str,
    code: &str,
) -> Result<Option<i64>, DbErr> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = ? AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
              ORDER BY rs.version_no DESC, rv.id LIMIT 1",
            [domain.into(), code.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "id").ok()))
}

/// Re-point one coding column from legacy `failure_codes` ids to `domain` reference values.
async fn remap_column<C: ConnectionTrait>(
    conn: &C,
    table: &str,
    column: &str,
    domain: &str,
    legacy_types: &[&str],
) -> Result<(), DbErr> {
    let ids = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT DISTINCT {column} AS v FROM {table} WHERE {column} IS NOT NULL"),
        ))
        .await?;
    for row in ids {
        let id: i64 = row.try_get("", "v")?;
        if reference_value_in_domain(conn, id, domain).await? {
            continue;
        }
        let legacy = conn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT code, label, code_type FROM failure_codes WHERE id = ?",
                [id.into()],
            ))
            .await?;
        let target = match legacy {
            Some(fc) => {
                let code: String = fc.try_get("", "code")?;
                let label: String = fc.try_get("", "label")?;
                let code_type: String = fc.try_get("", "code_type")?;
                if legacy_types.contains(&code_type.as_str()) {
                    if reference_value_id_by_code(conn, domain, &code).await?.is_none() {
                        ensure_value(conn, domain, &code, &label, None, None, 1000)
                            .await
                            .map_err(db_err)?;
                    }
                    reference_value_id_by_code(conn, domain, &code).await?
                } else {
                    None
                }
            }
            None => None,
        };
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!("UPDATE {table} SET {column} = ? WHERE {column} = ?"),
            [target.into(), id.into()],
        ))
        .await?;
    }
    Ok(())
}

async fn schema_sql<C: ConnectionTrait>(conn: &C, sql: &str) -> Result<Vec<(String, String)>, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get::<String>("", "name")?, r.try_get::<String>("", "sql")?)))
        .collect()
}

/// Rebuild `table` (SQLite cannot alter a foreign key) keeping columns, rows, ids, indexes and
/// triggers; `fk_columns` switch from `failure_codes(id)` to `reference_values(id)`. Views that
/// read the table are dropped and recreated around the rebuild.
async fn rebuild_with_reference_fks<C: ConnectionTrait>(
    conn: &C,
    table: &str,
    fk_columns: &[&str],
) -> Result<(), DbErr> {
    let rebuilt = format!("{table}_m178");
    let table_sql = schema_sql(
        conn,
        &format!("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name = '{table}'"),
    )
    .await?
    .into_iter()
    .next()
    .map(|(_, sql)| sql)
    .ok_or_else(|| DbErr::Custom(format!("{table} not found")))?;
    let indexes = schema_sql(
        conn,
        &format!(
            "SELECT name, sql FROM sqlite_master WHERE type = 'index' AND tbl_name = '{table}' AND sql IS NOT NULL"
        ),
    )
    .await?;
    let triggers = schema_sql(
        conn,
        &format!("SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = '{table}'"),
    )
    .await?;
    let views = schema_sql(
        conn,
        &format!("SELECT name, sql FROM sqlite_master WHERE type = 'view' AND sql LIKE '%{table}%'"),
    )
    .await?;

    let paren = table_sql
        .find('(')
        .ok_or_else(|| DbErr::Custom(format!("unexpected {table} definition")))?;
    let mut rebuilt_sql = format!("CREATE TABLE {rebuilt} {}", &table_sql[paren..]);
    for column in fk_columns {
        let from = format!("{column} INTEGER NULL REFERENCES failure_codes(id)");
        if !rebuilt_sql.contains(&from) {
            return Err(DbErr::Custom(format!("{table}.{column}: unexpected definition")));
        }
        rebuilt_sql = rebuilt_sql.replace(&from, &format!("{column} INTEGER NULL REFERENCES reference_values(id)"));
    }

    for (name, _) in &views {
        conn.execute_unprepared(&format!("DROP VIEW IF EXISTS \"{name}\""))
            .await?;
    }
    conn.execute_unprepared(&format!("DROP TABLE IF EXISTS {rebuilt}"))
        .await?;
    conn.execute_unprepared(&rebuilt_sql).await?;
    conn.execute_unprepared(&format!("INSERT INTO {rebuilt} SELECT * FROM {table}"))
        .await?;
    conn.execute_unprepared(&format!("DROP TABLE {table}")).await?;
    conn.execute_unprepared(&format!("ALTER TABLE {rebuilt} RENAME TO {table}"))
        .await?;
    for (_, sql) in &indexes {
        conn.execute_unprepared(sql).await?;
    }
    for (_, sql) in &triggers {
        conn.execute_unprepared(sql).await?;
    }
    for (_, sql) in &views {
        conn.execute_unprepared(sql).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // 1. Close-out catalogs (idempotent; also run at startup).
        ensure_closeout_reference_catalogs(db).await.map_err(db_err)?;

        // 2. Legacy failure_codes ids → reference values (same code).
        remap_column(db, TABLE, "failure_mode_id", MODES_DOMAIN, &["mode"]).await?;
        remap_column(
            db,
            TABLE,
            "failure_cause_id",
            DOMAIN_FAILURE_CAUSES,
            &["cause", "mechanism"],
        )
        .await?;
        remap_column(db, TABLE, "failure_effect_id", DOMAIN_FAILURE_EFFECTS, &["effect"]).await?;
        // A symptom id that is not a reference value cannot be shown or kept under the FK.
        db.execute_unprepared(&format!(
            "UPDATE {TABLE} SET symptom_id = NULL \
             WHERE symptom_id IS NOT NULL AND symptom_id NOT IN (SELECT id FROM reference_values)"
        ))
        .await?;
        // RAMS failure events are derived from the WO coding at close-out: same columns.
        remap_column(db, FAILURE_EVENTS, "failure_mode_id", MODES_DOMAIN, &["mode"]).await?;
        remap_column(
            db,
            FAILURE_EVENTS,
            "failure_cause_id",
            DOMAIN_FAILURE_CAUSES,
            &["cause"],
        )
        .await?;
        remap_column(
            db,
            FAILURE_EVENTS,
            "failure_effect_id",
            DOMAIN_FAILURE_EFFECTS,
            &["effect"],
        )
        .await?;
        remap_column(
            db,
            FAILURE_EVENTS,
            "failure_mechanism_id",
            DOMAIN_FAILURE_MECHANISMS,
            &["mechanism"],
        )
        .await?;

        // 3. Foreign keys on reference_values.
        rebuild_with_reference_fks(db, TABLE, &["failure_mode_id", "failure_cause_id", "failure_effect_id"]).await?;
        rebuild_with_reference_fks(
            db,
            FAILURE_EVENTS,
            &[
                "failure_mode_id",
                "failure_cause_id",
                "failure_effect_id",
                "failure_mechanism_id",
            ],
        )
        .await?;

        // 4. ISO 14224 mechanism and detection method on the WO coding.
        db.execute_unprepared(&format!(
            "ALTER TABLE {TABLE} ADD COLUMN failure_mechanism_id INTEGER NULL REFERENCES reference_values(id)"
        ))
        .await?;
        db.execute_unprepared(&format!(
            "ALTER TABLE {TABLE} ADD COLUMN detection_method_id INTEGER NULL REFERENCES reference_values(id)"
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Data-preserving forward fix: the previous foreign keys pointed at the wrong table.
        Ok(())
    }
}

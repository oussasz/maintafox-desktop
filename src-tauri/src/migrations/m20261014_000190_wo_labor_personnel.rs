//! Migration 190 — work-order labour booked against the personnel register.
//!
//! `work_order_interveners.intervener_id` was `NOT NULL REFERENCES user_accounts(id)`: only people
//! with an application login could be booked on a work order, so contractor technicians and
//! shop-floor staff without an account could not be recorded (SAP PM / Maximo book labour against
//! the personnel or labour record, not a login).
//!
//! - Rebuilds the table (SQLite cannot relax `NOT NULL`) with the same rows, ids, indexes and
//!   triggers: `intervener_id` becomes NULL-able and two columns are added:
//!   - `personnel_id INTEGER NULL REFERENCES personnel(id)` — the person who did the work;
//!   - `is_overtime INTEGER NOT NULL DEFAULT 0` — the hours are paid at the overtime rate.
//!   A CHECK keeps at least one of `intervener_id` / `personnel_id` set.
//! - Back-fills `personnel_id` from `user_accounts.personnel_id` for existing entries.
//!
//! `work_order_interveners` is tenant-synchronised; both new columns are additive and the new
//! foreign key is declared, so the sync engine maps it like the other personnel references.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000190_wo_labor_personnel"
    }
}

const TABLE: &str = "work_order_interveners";
const REBUILT: &str = "work_order_interveners_m190";

async fn schema_sql<C: ConnectionTrait>(conn: &C, sql: &str) -> Result<Vec<(String, String)>, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get::<String>("", "name")?, r.try_get::<String>("", "sql")?)))
        .collect()
}

async fn column_names<C: ConnectionTrait>(conn: &C, table: &str) -> Result<Vec<String>, DbErr> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    rows.iter().map(|r| r.try_get::<String>("", "name")).collect()
}

/// Rewrites the `intervener_id` column definition of `table_sql` to drop its `NOT NULL`.
fn relax_intervener_column(table_sql: &str) -> Result<String, DbErr> {
    let start = table_sql
        .find("intervener_id")
        .ok_or_else(|| DbErr::Custom(format!("{TABLE}.intervener_id: column not found")))?;
    let rest = &table_sql[start..];
    let end = rest
        .find(',')
        .ok_or_else(|| DbErr::Custom(format!("{TABLE}.intervener_id: unexpected definition")))?;
    let definition = &rest[..end];
    let normalized = definition.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized != "intervener_id INTEGER NOT NULL REFERENCES user_accounts(id)" {
        return Err(DbErr::Custom(format!(
            "{TABLE}.intervener_id: unexpected definition '{normalized}'"
        )));
    }
    Ok(format!(
        "{}intervener_id INTEGER NULL REFERENCES user_accounts(id){}",
        &table_sql[..start],
        &rest[end..]
    ))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let existing = column_names(db, TABLE).await?;
        if existing.iter().any(|c| c == "personnel_id") {
            return Ok(());
        }

        let table_sql = schema_sql(
            db,
            &format!("SELECT name, sql FROM sqlite_master WHERE type = 'table' AND name = '{TABLE}'"),
        )
        .await?
        .into_iter()
        .next()
        .map(|(_, sql)| sql)
        .ok_or_else(|| DbErr::Custom(format!("{TABLE} not found")))?;
        let indexes = schema_sql(
            db,
            &format!("SELECT name, sql FROM sqlite_master WHERE type = 'index' AND tbl_name = '{TABLE}' AND sql IS NOT NULL"),
        )
        .await?;
        let triggers = schema_sql(
            db,
            &format!("SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND tbl_name = '{TABLE}'"),
        )
        .await?;
        let views = schema_sql(
            db,
            &format!("SELECT name, sql FROM sqlite_master WHERE type = 'view' AND sql LIKE '%{TABLE}%'"),
        )
        .await?;

        let relaxed = relax_intervener_column(&table_sql)?;
        let paren = relaxed
            .find('(')
            .ok_or_else(|| DbErr::Custom(format!("unexpected {TABLE} definition")))?;
        let close = relaxed
            .rfind(')')
            .ok_or_else(|| DbErr::Custom(format!("unexpected {TABLE} definition")))?;
        let rebuilt_sql = format!(
            "CREATE TABLE {REBUILT} {},
                personnel_id INTEGER NULL REFERENCES personnel(id),
                is_overtime  INTEGER NOT NULL DEFAULT 0 CHECK (is_overtime IN (0, 1)),
                CHECK (intervener_id IS NOT NULL OR personnel_id IS NOT NULL)
            )",
            &relaxed[paren..close].trim_end()
        );

        for (name, _) in &views {
            db.execute_unprepared(&format!("DROP VIEW IF EXISTS \"{name}\""))
                .await?;
        }
        db.execute_unprepared(&format!("DROP TABLE IF EXISTS {REBUILT}"))
            .await?;
        db.execute_unprepared(&rebuilt_sql).await?;
        let columns = existing.join(", ");
        db.execute_unprepared(&format!(
            "INSERT INTO {REBUILT} ({columns}, personnel_id) \
             SELECT {}, (SELECT ua.personnel_id FROM user_accounts ua \
                          JOIN personnel p ON p.id = ua.personnel_id \
                          WHERE ua.id = {TABLE}.intervener_id) \
             FROM {TABLE}",
            existing
                .iter()
                .map(|c| format!("{TABLE}.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .await?;
        db.execute_unprepared(&format!("DROP TABLE {TABLE}")).await?;
        db.execute_unprepared(&format!("ALTER TABLE {REBUILT} RENAME TO {TABLE}"))
            .await?;
        for (_, sql) in &indexes {
            db.execute_unprepared(sql).await?;
        }
        db.execute_unprepared(&format!(
            "CREATE INDEX IF NOT EXISTS idx_woi_personnel_id ON {TABLE}(personnel_id)"
        ))
        .await?;
        for (_, sql) in &triggers {
            db.execute_unprepared(sql).await?;
        }
        for (_, sql) in &views {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Entries booked for people without a login cannot be put back under NOT NULL.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::relax_intervener_column;

    #[test]
    fn relaxes_only_the_intervener_column() {
        let sql = "CREATE TABLE work_order_interveners (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
                intervener_id   INTEGER NOT NULL REFERENCES user_accounts(id),
                skill_id        INTEGER NULL
            )";
        let out = relax_intervener_column(sql).expect("relaxed");
        assert!(out.contains("intervener_id INTEGER NULL REFERENCES user_accounts(id),"));
        assert!(out.contains("work_order_id   INTEGER NOT NULL REFERENCES work_orders(id)"));
    }

    #[test]
    fn refuses_an_unexpected_definition() {
        let sql = "CREATE TABLE work_order_interveners (id INTEGER, intervener_id TEXT, x INTEGER)";
        assert!(relax_intervener_column(sql).is_err());
    }
}

//! The rows a new database holds before any seeder runs: every row the migrations create (system
//! roles, permissions, statuses, catalogs, lists, plant settings).
//!
//! The local data reset of a first activation (`product_license::reset_local_tenant_runtime_data`)
//! wipes the tenant tables but does not run the migrations again (`seaql_migrations` is kept), and
//! the seeders put back only their own rows. Rows a migration created never came back, and sync
//! never sends them (they have the same identity on every device), so whatever referenced them
//! waited forever on a joining PC. The reset now takes them from a database built here the way a
//! new installation builds it (`Migrator::up` on an empty in-memory database): the migrations stay
//! the only definition of that baseline, with no list kept by hand.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::errors::{AppError, AppResult};

/// Rows per `INSERT` when a table is copied.
const ROWS_PER_INSERT: usize = 200;

/// A private in-memory database on a single connection that lives as long as the pool, opened
/// like `Database::connect("sqlite::memory:")` does (sqlx defaults, `foreign_keys = ON`).
pub(crate) async fn empty_memory_db() -> AppResult<DatabaseConnection> {
    let opts = SqliteConnectOptions::new().in_memory(true).shared_cache(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(opts)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("in-memory database: {e}")))?;
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
    db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
    Ok(db)
}

/// A new database with every migration applied and nothing else.
pub struct MigrationBaseline {
    db: DatabaseConnection,
}

impl MigrationBaseline {
    /// Builds it. Seconds at most; run before anything is deleted, so a failure changes nothing.
    pub async fn build() -> AppResult<Self> {
        use sea_orm_migration::MigratorTrait;
        let db = empty_memory_db().await?;
        crate::migrations::Migrator::up(&db, None).await?;
        Ok(Self { db })
    }

    /// Inserts into `target`'s `table` the rows the baseline holds in that table, all of them or
    /// those `filter` (SQL over the table's columns, written in code) selects. `target` must not
    /// hold them already (the reset has just deleted them): a row that does not fit is an error,
    /// never skipped.
    ///
    /// The whole table keeps the baseline's row ids (rows refer to each other by id). A `filter`
    /// copies part of a table whose other rows are kept: the row id is left to `target`, where the
    /// baseline's could be held by a kept row.
    ///
    /// Returns the number of rows inserted; a table the baseline does not have gives 0.
    pub async fn restore_into(
        &self,
        target: &impl ConnectionTrait,
        table: &str,
        filter: Option<&str>,
    ) -> AppResult<u64> {
        let baseline_columns = columns(&self.db, table).await?;
        if baseline_columns.is_empty() {
            return Ok(0);
        }
        let target_columns = columns(target, table).await?;
        let mut copied = Vec::with_capacity(baseline_columns.len());
        for column in &baseline_columns {
            if filter.is_some() && column.integer_key {
                continue;
            }
            if !target_columns.iter().any(|c| c.name == column.name) {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "baseline restore: {table}.{} is not in this database",
                    column.name
                )));
            }
            copied.push(quote_identifier(&column.name));
        }

        // Each row as SQL literals (`quote`), so every type (text, numbers, blobs, NULL) is
        // written back as stored.
        let literals = copied
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let rows = self
            .db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!(
                    "SELECT {literals} AS v FROM {} WHERE {} ORDER BY rowid",
                    quote_identifier(table),
                    filter.unwrap_or("1 = 1")
                ),
            ))
            .await?;
        let insert = format!(
            "INSERT INTO {} ({}) VALUES ",
            quote_identifier(table),
            copied.join(", ")
        );
        let mut inserted: u64 = 0;
        for chunk in rows.chunks(ROWS_PER_INSERT) {
            let mut values = Vec::with_capacity(chunk.len());
            for row in chunk {
                values.push(format!("({})", row.try_get::<String>("", "v")?));
            }
            let result = target
                .execute(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("{insert}{}", values.join(", ")),
                ))
                .await?;
            inserted = inserted.saturating_add(result.rows_affected());
        }
        Ok(inserted)
    }
}

struct Column {
    name: String,
    /// The table's `INTEGER PRIMARY KEY` (its row id).
    integer_key: bool,
}

/// The stored columns of `table` (generated columns excluded); empty when there is no such table.
async fn columns(conn: &impl ConnectionTrait, table: &str) -> AppResult<Vec<Column>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT name, pk, upper(type) AS type, (SELECT count(*) FROM pragma_table_info(?) WHERE pk > 0) AS pk_columns \
               FROM pragma_table_info(?) ORDER BY cid",
            [table.into(), table.into()],
        ))
        .await?;
    let mut columns = Vec::with_capacity(rows.len());
    for row in rows {
        let pk: i64 = row.try_get("", "pk")?;
        let ty: String = row.try_get("", "type")?;
        let pk_columns: i64 = row.try_get("", "pk_columns")?;
        columns.push(Column {
            name: row.try_get("", "name")?,
            integer_key: pk == 1 && pk_columns == 1 && ty == "INTEGER",
        });
    }
    Ok(columns)
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub mod demo_seeder;
pub mod encryption;
pub mod integrity;
pub mod migration_integrity;
pub mod org_schema;
/// DEMO-ONLY RAMS seed modules — never call from production app flow.
pub mod rams_presentation_seed;
pub mod rams_sql_demo_seed;
pub mod reference_domains;
pub mod schema_integration_test;
pub mod seeder;
pub mod tenant_bootstrap;
#[cfg(test)]
pub(crate) mod test_db;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use std::path::Path;
use std::time::Duration;

use crate::errors::{AppError, AppResult};

/// The row id of the row the `INSERT` that gave `result` added, read from that statement's own
/// result. A separate `SELECT last_insert_rowid()` on the pool is a second statement: another task
/// can run its own insert on the connection in between, and the id read then names that other row
/// (or none of this table). Inside a transaction both forms are safe; this one is safe everywhere.
pub fn inserted_id(result: &sea_orm::ExecResult) -> AppResult<i64> {
    i64::try_from(result.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("inserted row id out of range")))
}

/// Opens a single-connection pool on `path` through SQLCipher.
///
/// `key = None` opens a plaintext file (only used to migrate legacy databases and in tests).
/// Statement logging is always disabled: the connection setup executes `PRAGMA key`.
pub(crate) async fn connect_sqlcipher(
    path: &Path,
    key: Option<&encryption::DatabaseKey>,
    create_if_missing: bool,
) -> AppResult<DatabaseConnection> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::ConnectOptions;

    let mut opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create_if_missing)
        .disable_statement_logging();
    if let Some(key) = key {
        // sqlx always emits `key` as the first PRAGMA on a new connection, as SQLCipher requires.
        opts = opts.pragma("key", key.pragma_value());
    }
    // SQLite: use a single connection so PRAGMA settings (e.g. foreign_keys off during tenant reset)
    // apply to the same connection that runs transactions. Multi-connection pools can otherwise
    // leave FK enforcement enabled during DELETE and trigger error 787.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(120))
        .connect_with(opts)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("cannot open database {}: {e}", path.display())))?;
    Ok(sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

/// Fails unless the linked SQLite is SQLCipher: without it `PRAGMA key` is silently ignored and the
/// database would stay plaintext.
async fn assert_sqlcipher_linked(db: &DatabaseConnection) -> AppResult<()> {
    let version = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA cipher_version".to_string(),
        ))
        .await?
        .and_then(|row| row.try_get::<String>("", "cipher_version").ok());
    match version {
        Some(v) if !v.trim().is_empty() => Ok(()),
        _ => Err(AppError::Internal(anyhow::anyhow!(
            "this build is not linked against SQLCipher; refusing to store data unencrypted"
        ))),
    }
}

/// Initialize the encrypted `SQLite` database connection and apply WAL + performance pragmas.
///
/// `db_path` is the absolute path to the `.db` file.
/// The directory must already exist before calling this function.
pub async fn init_db(db_path: &str) -> AppResult<DatabaseConnection> {
    tracing::info!("Opening database: {}", db_path);
    let path = Path::new(db_path);

    let key = encryption::prepare_database_key(path).await?;
    let db = connect_sqlcipher(path, Some(&key), true).await?;
    assert_sqlcipher_linked(&db).await?;
    db.query_one(Statement::from_string(
        DatabaseBackend::Sqlite,
        "SELECT count(*) AS n FROM sqlite_master".to_string(),
    ))
    .await
    .map_err(|_| {
        AppError::DatabaseKeyUnavailable("the stored key does not open the database; enter the recovery key".into())
    })?;

    // Apply SQLite pragmas for WAL mode, foreign keys, and performance
    db.execute_unprepared("PRAGMA journal_mode=WAL;").await?;
    db.execute_unprepared("PRAGMA foreign_keys=ON;").await?;
    db.execute_unprepared("PRAGMA busy_timeout=5000;").await?;
    db.execute_unprepared("PRAGMA cache_size=-20000;").await?;
    db.execute_unprepared("PRAGMA temp_store=MEMORY;").await?;

    tracing::info!("Database connection established with WAL mode.");
    Ok(db)
}

/// Run all pending sea-orm migrations.
pub async fn run_migrations(db: &DatabaseConnection) -> AppResult<()> {
    use sea_orm_migration::MigratorTrait;
    tracing::info!("Running pending database migrations...");
    crate::migrations::Migrator::up(db, None).await?;
    tracing::info!("Migrations complete.");
    Ok(())
}

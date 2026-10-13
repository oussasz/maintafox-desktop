//! Shared test helpers for SP04-F04 integration tests.

use std::str::FromStr;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, SqlxSqliteConnector, Statement};
use sea_orm_migration::MigratorTrait;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// A fresh in-memory SQLite database served by one connection that is never closed (same as the
/// library's `db::test_db::connect_in_memory`, which integration tests cannot reach): a pooled
/// `sqlite::memory:` database vanishes when the pool drops its connections.
async fn connect_in_memory() -> DatabaseConnection {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:").expect("in-memory SQLite options");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(opts)
        .await
        .expect("in-memory SQLite should connect");
    SqlxSqliteConnector::from_sqlx_sqlite_pool(pool)
}

/// Create an in-memory SQLite database with all migrations and seed data applied.
pub async fn create_test_db() -> DatabaseConnection {
    let db = connect_in_memory().await;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA foreign_keys = ON;".to_string(),
    ))
    .await
    .expect("PRAGMA foreign_keys");

    maintafox_lib::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations should apply cleanly");

    maintafox_lib::db::seeder::seed_system_data(&db)
        .await
        .expect("seeder should run cleanly");

    db
}

//! In-memory SQLite for tests.
//!
//! Connecting sea-orm to `sqlite::memory:` opens a pool whose in-memory database lives only as long
//! as one of the pool's connections does; when the pool closes idle or aged connections (or opens
//! a second one before the first exists) tests see an empty database ("no such table: ...").
//! This helper keeps exactly one connection open for the life of the handle, like
//! [`super::connect_sqlcipher`] does for the app's database file.

use sea_orm::{DatabaseConnection, SqlxSqliteConnector};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

/// A fresh, empty in-memory SQLite database served by a single connection that is never closed.
pub async fn connect_in_memory() -> Result<DatabaseConnection, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(opts)
        .await?;
    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

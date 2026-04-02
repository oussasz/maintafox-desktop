pub mod org_schema;
pub mod reference_domains;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;

use crate::errors::AppResult;

/// Initialize the SQLite database connection and apply WAL + performance pragmas.
///
/// `db_path` is the absolute path to the `.db` file.
/// The directory must already exist before calling this function.
pub async fn init_db(db_path: &str) -> AppResult<DatabaseConnection> {
    tracing::info!("Opening database: {}", db_path);

    let url = format!("sqlite://{}?mode=rwc", db_path);

    let mut opts = ConnectOptions::new(url);
    opts.max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(120))
        .sqlx_logging(
            std::env::var("MAINTAFOX_SQL_LOG")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        );

    let db = Database::connect(opts).await?;

    // Apply SQLite pragmas for WAL mode, foreign keys, and performance
    use sea_orm::ConnectionTrait;
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

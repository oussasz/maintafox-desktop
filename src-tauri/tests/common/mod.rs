//! Shared test helpers for SP04-F04 integration tests.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

/// Create an in-memory SQLite database with all migrations and seed data applied.
pub async fn create_test_db() -> DatabaseConnection {
    let db = sea_orm::Database::connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should connect");

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

//! Migration 041 - Personnel import staging and report permission.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    "m20260417_000041_personnel_import_and_reports"
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared(
      "CREATE TABLE IF NOT EXISTS personnel_import_batches (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        source_filename TEXT NOT NULL,
        source_sha256 TEXT NOT NULL,
        source_kind TEXT NOT NULL,
        mode TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'uploaded',
        total_rows INTEGER NOT NULL DEFAULT 0,
        valid_rows INTEGER NOT NULL DEFAULT 0,
        warning_rows INTEGER NOT NULL DEFAULT 0,
        error_rows INTEGER NOT NULL DEFAULT 0,
        initiated_by_id INTEGER NULL REFERENCES user_accounts(id),
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
      )"
    ).await?;

    db.execute_unprepared(
      "CREATE INDEX IF NOT EXISTS idx_per_import_batches_status ON personnel_import_batches(status)"
    ).await?;

    db.execute_unprepared(
      "CREATE INDEX IF NOT EXISTS idx_per_import_batches_created_at ON personnel_import_batches(created_at DESC)"
    ).await?;

    db.execute_unprepared(
      "CREATE TABLE IF NOT EXISTS personnel_import_rows (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        batch_id INTEGER NOT NULL REFERENCES personnel_import_batches(id) ON DELETE CASCADE,
        row_no INTEGER NOT NULL,
        raw_json TEXT NOT NULL,
        employee_code TEXT NULL,
        hr_external_id TEXT NULL,
        target_personnel_id INTEGER NULL REFERENCES personnel(id),
        target_row_version INTEGER NULL,
        validation_status TEXT NOT NULL DEFAULT 'pending',
        messages_json TEXT NOT NULL DEFAULT '[]',
        proposed_action TEXT NULL,
        normalized_json TEXT NOT NULL DEFAULT '{}'
      )"
    ).await?;

    db.execute_unprepared(
      "CREATE INDEX IF NOT EXISTS idx_per_import_rows_batch ON personnel_import_rows(batch_id, row_no)"
    ).await?;

    db.execute_unprepared(
      "CREATE INDEX IF NOT EXISTS idx_per_import_rows_status ON personnel_import_rows(validation_status)"
    ).await?;

    let now = chrono::Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
      DbBackend::Sqlite,
      "INSERT OR IGNORE INTO permissions
      (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
      VALUES ('per.report', 'View and export workforce reports', 'personnel', 0, 0, 1, ?)",
      [now.clone().into()],
    )).await?;

    db.execute(Statement::from_sql_and_values(
      DbBackend::Sqlite,
      "INSERT OR IGNORE INTO permission_dependencies
      (permission_name, required_permission_name, dependency_type, created_at)
      VALUES ('per.report', 'per.view', 'hard', ?)",
      [now.into()],
    )).await?;

    Ok(())
  }

  async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();

    db.execute_unprepared("DROP TABLE IF EXISTS personnel_import_rows").await?;
    db.execute_unprepared("DROP TABLE IF EXISTS personnel_import_batches").await?;

    Ok(())
  }
}

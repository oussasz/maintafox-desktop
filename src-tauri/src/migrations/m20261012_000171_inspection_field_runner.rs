//! Migration 171 — Inspection field runner (PRD §6.25, Phase 6.2).
//!
//! - `inspection_checkpoints.description`: the instruction a technician reads in the field
//!   (the code alone is not a field-grade label).
//! - `inspection_rounds`: lifecycle stamps (`started_at`, `completed_at`, `reviewed_at`),
//!   the reviewer (`reviewed_by_id` → `user_accounts`), the review note and the completion
//!   percentage computed when the round is completed.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000171_inspection_field_runner"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("inspection_checkpoints", "description", "TEXT NULL"),
    ("inspection_rounds", "started_at", "TEXT NULL"),
    ("inspection_rounds", "completed_at", "TEXT NULL"),
    ("inspection_rounds", "reviewed_at", "TEXT NULL"),
    (
        "inspection_rounds",
        "reviewed_by_id",
        "INTEGER NULL REFERENCES user_accounts(id)",
    ),
    ("inspection_rounds", "review_note", "TEXT NULL"),
    ("inspection_rounds", "completion_percentage", "REAL NULL"),
];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (table, column, ddl) in COLUMNS {
            if !column_exists(db, table, column).await? {
                db.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive, nullable columns only.
        Ok(())
    }
}

//! Migration 027 — Add `conclusion` column to `work_orders`.
//!
//! Phase 2 - Sub-phase 05 - File 04 - Sprint S1 (post-audit fix).
//!
//! The `WoMechCompleteInput` struct was extended with an optional `conclusion`
//! text field (technician's free-text conclusion at mechanical completion), but
//! the corresponding DDL column was missing from migration 022.
//! This migration adds it non-destructively with ALTER TABLE.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260411_000027_wo_conclusion_column"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("work_orders"))
                    .add_column(ColumnDef::new(Alias::new("conclusion")).text())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite does not support DROP COLUMN in older versions — use a no-op
        // The column is optional (NULL) and harmless if left in place.
        let _ = manager;
        Ok(())
    }
}

//! Migration 009 — Append-only audit table for org structural changes.
//!
//! Sub-phase 01 — File 04 — Sprint S2.
//! This table records every structural org change: publish, move, deactivate,
//! responsibility assignment, entity binding, and blocked validation attempts.
//! Rows are immutable once written — no UPDATE or DELETE path exists.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260406_000009_org_audit_trail"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("org_change_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("entity_kind")).text().not_null())
                    .col(ColumnDef::new(Alias::new("entity_id")).integer())
                    .col(ColumnDef::new(Alias::new("change_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("before_json")).text())
                    .col(ColumnDef::new(Alias::new("after_json")).text())
                    .col(ColumnDef::new(Alias::new("preview_summary_json")).text())
                    .col(ColumnDef::new(Alias::new("changed_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("changed_at")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("requires_step_up"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("apply_result"))
                            .text()
                            .not_null()
                            .default("applied"),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("org_change_events")).to_owned())
            .await?;
        Ok(())
    }
}

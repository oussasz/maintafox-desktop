//! Migration 068 — permit suspensions and handover logs (PRD §6.23 sprint 02).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260616_000068_permit_lifecycle_subentities"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permit_suspensions"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("entity_sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("permit_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("reason")).text().not_null())
                    .col(ColumnDef::new(Alias::new("suspended_by_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("suspended_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("reinstated_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("reinstated_at")).text())
                    .col(ColumnDef::new(Alias::new("reactivation_conditions")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_permit_suspensions_permit")
                    .table(Alias::new("permit_suspensions"))
                    .col(Alias::new("permit_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permit_handover_logs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("entity_sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("permit_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("handed_from_role")).text().not_null())
                    .col(ColumnDef::new(Alias::new("handed_to_role")).text().not_null())
                    .col(ColumnDef::new(Alias::new("confirmation_note")).text().not_null())
                    .col(ColumnDef::new(Alias::new("signed_at")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_permit_handover_logs_permit")
                    .table(Alias::new("permit_handover_logs"))
                    .col(Alias::new("permit_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("permit_handover_logs")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("permit_suspensions")).to_owned())
            .await?;
        Ok(())
    }
}

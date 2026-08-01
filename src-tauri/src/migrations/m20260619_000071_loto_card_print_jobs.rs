//! Migration 071 — LOTO card print audit + optional lock tag on isolations (PRD §6.23 sprint 04).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260619_000071_loto_card_print_jobs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE permit_isolations ADD COLUMN lock_number TEXT NULL")
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Alias::new("loto_card_print_jobs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("permit_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("isolation_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("printed_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("printed_by_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("entity_sync_id")).text().not_null().unique_key())
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
                    .name("idx_loto_card_print_jobs_permit")
                    .table(Alias::new("loto_card_print_jobs"))
                    .col(Alias::new("permit_id"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_loto_card_print_jobs_isolation")
                    .table(Alias::new("loto_card_print_jobs"))
                    .col(Alias::new("isolation_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("loto_card_print_jobs")).to_owned())
            .await?;
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE permit_isolations DROP COLUMN lock_number")
            .await?;
        Ok(())
    }
}

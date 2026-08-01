//! Migration 067 — LOTO / PTW permit domain (PRD §6.23, gap sprint 01).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260615_000067_permit_domain_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permit_types"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("entity_sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("description")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("requires_hse_approval"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("requires_operations_approval"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("requires_atmospheric_test"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("max_duration_hours")).double())
                    .col(
                        ColumnDef::new(Alias::new("mandatory_ppe_ids_json"))
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("mandatory_control_rules_json"))
                            .text()
                            .not_null()
                            .default("{}"),
                    )
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
            .create_table(
                Table::create()
                    .table(Alias::new("work_permits"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("entity_sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("linked_work_order_id")).integer())
                    .col(ColumnDef::new(Alias::new("permit_type_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("asset_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("entity_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("status")).text().not_null())
                    .col(ColumnDef::new(Alias::new("requested_at")).text())
                    .col(ColumnDef::new(Alias::new("issued_at")).text())
                    .col(ColumnDef::new(Alias::new("activated_at")).text())
                    .col(ColumnDef::new(Alias::new("expires_at")).text())
                    .col(ColumnDef::new(Alias::new("closed_at")).text())
                    .col(ColumnDef::new(Alias::new("handed_back_at")).text())
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
            .create_table(
                Table::create()
                    .table(Alias::new("permit_isolations"))
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
                    .col(ColumnDef::new(Alias::new("isolation_point")).text().not_null())
                    .col(ColumnDef::new(Alias::new("energy_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("isolation_method")).text().not_null())
                    .col(ColumnDef::new(Alias::new("applied_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("verified_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("applied_at")).text())
                    .col(ColumnDef::new(Alias::new("verified_at")).text())
                    .col(ColumnDef::new(Alias::new("removal_verified_at")).text())
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
                    .name("idx_work_permits_type")
                    .table(Alias::new("work_permits"))
                    .col(Alias::new("permit_type_id"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_work_permits_asset")
                    .table(Alias::new("work_permits"))
                    .col(Alias::new("asset_id"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_work_permits_status")
                    .table(Alias::new("work_permits"))
                    .col(Alias::new("status"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_work_permits_linked_wo")
                    .table(Alias::new("work_permits"))
                    .col(Alias::new("linked_work_order_id"))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_permit_isolations_permit")
                    .table(Alias::new("permit_isolations"))
                    .col(Alias::new("permit_id"))
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        db.execute_unprepared(
            "INSERT INTO permit_types (entity_sync_id, code, name, description, requires_hse_approval, \
             requires_operations_approval, requires_atmospheric_test, max_duration_hours, \
             mandatory_ppe_ids_json, mandatory_control_rules_json, row_version) VALUES \
             ('11111111-1111-4111-8111-111111111111', 'loto', 'LOTO', 'Lockout/Tagout', 1, 1, 0, 24.0, '[]', '{}', 1), \
             ('22222222-2222-4222-8222-222222222222', 'hot_work', 'Hot work', 'Hot work', 1, 1, 0, 8.0, '[]', '{}', 1), \
             ('33333333-3333-4333-8333-333333333333', 'confined_space', 'Confined space', 'Confined space', 1, 1, 1, 4.0, '[]', '{}', 1), \
             ('44444444-4444-4444-8444-444444444444', 'cold_work', 'Cold work', 'Cold work', 0, 1, 0, 12.0, '[]', '{}', 1), \
             ('55555555-5555-4555-8555-555555555555', 'electrical_lv', 'Electrical LV', 'Low voltage electrical', 1, 1, 0, 8.0, '[]', '{}', 1)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("permit_isolations")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("work_permits")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("permit_types")).to_owned())
            .await?;
        Ok(())
    }
}

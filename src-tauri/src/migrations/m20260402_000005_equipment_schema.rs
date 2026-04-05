// src-tauri/src/migrations/m20260402_000005_equipment_schema.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260402_000005_equipment_schema"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── equipment_classes ─────────────────────────────────────────────────
        // Governed classification hierarchy: class > family > subfamily.
        // This is a two-level self-referencing table (parent_id NULL = top-level class).
        // equipment_class_domain_id references the lookup_domains.id for the
        // "equipment.class" domain which validates canonical class codes.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_classes"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("code")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("parent_id")).integer())
                    // "class" | "family" | "subfamily"
                    .col(ColumnDef::new(Alias::new("level")).text().not_null().default("class"))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("erp_reference")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .to_owned(),
            )
            .await?;

        // ── equipment ─────────────────────────────────────────────────────────
        // Core equipment identity record. Every field that might be needed by
        // work orders, planning, PM cycles, reliability, ERP, or IoT is present.
        // The asset_id_code is the internal human tag (e.g. "POMPE-001").
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(
                        ColumnDef::new(Alias::new("asset_id_code"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("class_id")).integer())
                    // "active_in_service" | "in_stock" | "under_maintenance" | "decommissioned" |
                    // "scrapped" | "transferred" | "spare"
                    .col(
                        ColumnDef::new(Alias::new("lifecycle_status"))
                            .text()
                            .not_null()
                            .default("active_in_service"),
                    )
                    // criticality references lookup_values.id from domain "equipment.criticality"
                    .col(ColumnDef::new(Alias::new("criticality_value_id")).integer())
                    // primary org node this asset is installed at
                    .col(ColumnDef::new(Alias::new("installed_at_node_id")).integer())
                    // the functional position this asset occupies (nullable — for installed components)
                    .col(ColumnDef::new(Alias::new("functional_position_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("manufacturer")).text())
                    .col(ColumnDef::new(Alias::new("model")).text())
                    .col(ColumnDef::new(Alias::new("serial_number")).text())
                    .col(ColumnDef::new(Alias::new("purchase_date")).text())
                    .col(ColumnDef::new(Alias::new("commissioning_date")).text())
                    .col(ColumnDef::new(Alias::new("warranty_expiry_date")).text())
                    .col(ColumnDef::new(Alias::new("replacement_value")).double())
                    .col(ColumnDef::new(Alias::new("cost_center_code")).text())
                    // ERP asset number for reconciliation
                    .col(ColumnDef::new(Alias::new("erp_asset_id")).text())
                    // SAP functional location reference
                    .col(ColumnDef::new(Alias::new("erp_functional_location")).text())
                    // IoT asset identifier for signal binding
                    .col(ColumnDef::new(Alias::new("iot_asset_id")).text())
                    .col(ColumnDef::new(Alias::new("qr_code")).text())
                    .col(ColumnDef::new(Alias::new("barcode")).text())
                    .col(ColumnDef::new(Alias::new("photo_path")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("technical_specs_json")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .col(ColumnDef::new(Alias::new("last_synced_checkpoint")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_class_id")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("class_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_node_id")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("installed_at_node_id"))
                    .to_owned(),
            )
            .await?;

        // ── equipment_hierarchy ────────────────────────────────────────────────
        // Parent-child relationships between equipment items.
        // Models assemblies, sub-components, and installed parts.
        // relationship_type: "parent_child" | "installed_in" | "drives" | "feeds"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_hierarchy"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("parent_equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("child_equipment_id")).integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("relationship_type"))
                            .text()
                            .not_null()
                            .default("parent_child"),
                    )
                    .col(ColumnDef::new(Alias::new("position_label")).text())
                    .col(ColumnDef::new(Alias::new("installed_at")).text())
                    .col(ColumnDef::new(Alias::new("removed_at")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── equipment_meters ──────────────────────────────────────────────────
        // Meters on equipment used by PM cycle triggers and reliability analytics.
        // meter_type: "hours" | "cycles" | "distance" | "volume" | "custom"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_meters"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("meter_type"))
                            .text()
                            .not_null()
                            .default("hours"),
                    )
                    .col(ColumnDef::new(Alias::new("unit")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("current_reading"))
                            .double()
                            .not_null()
                            .default(0.0),
                    )
                    .col(ColumnDef::new(Alias::new("last_read_at")).text())
                    .col(ColumnDef::new(Alias::new("expected_rate_per_day")).double())
                    .col(ColumnDef::new(Alias::new("is_primary")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── equipment_lifecycle_events ─────────────────────────────────────────
        // Append-only log of significant lifecycle events on the asset.
        // event_type: "moved" | "installed" | "replaced" | "reclassified" |
        //             "decommissioned" | "reactivated" | "warranty_update"
        // This table is the provenance record for PRD §6.3 governance rules.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("equipment_lifecycle_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("equipment_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("event_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("occurred_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("performed_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("from_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("to_node_id")).integer())
                    .col(ColumnDef::new(Alias::new("from_status")).text())
                    .col(ColumnDef::new(Alias::new("to_status")).text())
                    .col(ColumnDef::new(Alias::new("related_work_order_id")).integer())
                    .col(ColumnDef::new(Alias::new("details_json")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    // lifecycle events are append-only: no updated_at, no deleted_at
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_eq_lifecycle_equipment_id")
                    .table(Alias::new("equipment_lifecycle_events"))
                    .col(Alias::new("equipment_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in [
            "equipment_lifecycle_events",
            "equipment_meters",
            "equipment_hierarchy",
            "equipment",
            "equipment_classes",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}

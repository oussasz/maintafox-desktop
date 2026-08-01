//! Migration 010 — Governed asset identity layer.
//!
//! Phase 2 - Sub-phase 02 - File 01 - Sprint S1.
//!
//! Extends the existing `equipment` table (migration 005) with governed identity
//! columns required by the asset registry backbone:
//!   - `maintainable_boundary` — explicit flag for reliability boundary separation
//!   - `decommissioned_at` — required when lifecycle_status = DECOMMISSIONED
//!
//! Creates `asset_external_ids` for tracking cross-system identifiers
//! (ERP asset numbers, SAP functional locations, legacy codes) with temporal validity.
//!
//! Adds effective dating to `equipment_hierarchy` for governed relationship lifecycle.
//!
//! Adds missing indexes for status and hierarchy query paths.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000010_asset_registry_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Extend equipment with governed identity columns ────────────
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN maintainable_boundary INTEGER NOT NULL DEFAULT 1",
        )
        .await?;

        db.execute_unprepared("ALTER TABLE equipment ADD COLUMN decommissioned_at TEXT")
            .await?;

        // ── 2. Create asset_external_ids ──────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_external_ids"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("asset_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("system_code")).text().not_null())
                    .col(ColumnDef::new(Alias::new("external_id")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("is_primary"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_asset_external_ids_asset")
                    .table(Alias::new("asset_external_ids"))
                    .col(Alias::new("asset_id"))
                    .to_owned(),
            )
            .await?;

        // ── 3. Extend equipment_hierarchy with effective dating ───────────
        db.execute_unprepared(
            "ALTER TABLE equipment_hierarchy ADD COLUMN effective_from TEXT",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_hierarchy ADD COLUMN effective_to TEXT",
        )
        .await?;

        // ── 4. Add missing indexes ────────────────────────────────────────
        // idx_equipment_node_id already exists from migration 005.
        // Add lifecycle_status index for status-filtered queries.
        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_lifecycle_status")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("lifecycle_status"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_hierarchy_parent")
                    .table(Alias::new("equipment_hierarchy"))
                    .col(Alias::new("parent_equipment_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_hierarchy_child")
                    .table(Alias::new("equipment_hierarchy"))
                    .col(Alias::new("child_equipment_id"))
                    .to_owned(),
            )
            .await?;

        // ── 5. Add maintainable_boundary index (for analytics queries) ────
        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_maintainable")
                    .table(Alias::new("equipment"))
                    .col(Alias::new("maintainable_boundary"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // SQLite does not support DROP COLUMN before 3.35.0.
        // For the down path we only drop the new table and indexes;
        // the ALTER-added columns remain (acceptable in a development context).
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_external_ids"))
                    .to_owned(),
            )
            .await?;

        // Drop indexes added in this migration
        for idx in [
            "idx_equipment_lifecycle_status",
            "idx_equipment_hierarchy_parent",
            "idx_equipment_hierarchy_child",
            "idx_equipment_maintainable",
        ] {
            db.execute_unprepared(&format!("DROP INDEX IF EXISTS {idx}"))
                .await?;
        }

        Ok(())
    }
}

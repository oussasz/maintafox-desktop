//! Migration 011 — Asset lifecycle history, meter readings, and document links.
//!
//! Phase 2 - Sub-phase 02 - File 02.
//!
//! Sprint S1 scope:
//!   Extends the existing `equipment_lifecycle_events` table (migration 005)
//!   with columns required for governed lifecycle evidence:
//!     - `from_class_code` / `to_class_code` — reclassification tracking
//!     - `related_asset_id` — replacement counterpart linkage
//!     - `reason_code` — governed reason classification
//!     - `approved_by_id` — approval chain traceability
//!   Adds composite indexes for analytical query paths.
//!
//! Sprint S2 scope:
//!   Extends the existing `equipment_meters` table (migration 005) with
//!   `meter_code` (governed code identifier) and `rollover_value` (for
//!   cyclic counters). Creates the new `asset_meter_readings` table for
//!   append-only time-series readings with source tracking and quality flags.
//!
//! Sprint S3 scope:
//!   Creates `asset_document_links` table for governed document references
//!   with purpose codes, primary-link semantics, and validity periods.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000011_asset_lifecycle_meter_docs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Extend equipment_lifecycle_events with governed columns ────
        //
        // Existing columns (migration 005):
        //   id, sync_id, equipment_id, event_type, occurred_at,
        //   performed_by_id, from_node_id, to_node_id, from_status,
        //   to_status, related_work_order_id, details_json, notes,
        //   created_at, origin_machine_id
        //
        // New columns:
        db.execute_unprepared(
            "ALTER TABLE equipment_lifecycle_events ADD COLUMN from_class_code TEXT",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_lifecycle_events ADD COLUMN to_class_code TEXT",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_lifecycle_events ADD COLUMN related_asset_id INTEGER",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_lifecycle_events ADD COLUMN reason_code TEXT",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_lifecycle_events ADD COLUMN approved_by_id INTEGER",
        )
        .await?;

        // ── 2. Add composite index for timeline queries ───────────────────
        //
        // Migration 005 created `idx_eq_lifecycle_equipment_id` on (equipment_id).
        // We add a composite for (equipment_id, occurred_at) to optimize
        // timeline scans and an index on event_type for type-filtered queries.
        manager
            .create_index(
                Index::create()
                    .name("idx_eq_lifecycle_equip_occurred")
                    .table(Alias::new("equipment_lifecycle_events"))
                    .col(Alias::new("equipment_id"))
                    .col(Alias::new("occurred_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_eq_lifecycle_event_type")
                    .table(Alias::new("equipment_lifecycle_events"))
                    .col(Alias::new("event_type"))
                    .to_owned(),
            )
            .await?;

        // ── 3. Add index for replacement counterpart lookups ──────────────
        manager
            .create_index(
                Index::create()
                    .name("idx_eq_lifecycle_related_asset")
                    .table(Alias::new("equipment_lifecycle_events"))
                    .col(Alias::new("related_asset_id"))
                    .to_owned(),
            )
            .await?;

        // ══════════════════════════════════════════════════════════════════
        // Sprint S2 — Meter and reading governance
        // ══════════════════════════════════════════════════════════════════

        // ── 4. Extend equipment_meters with governed columns ──────────────
        //
        // Existing columns (migration 005):
        //   id, sync_id, equipment_id, name, meter_type, unit,
        //   current_reading, last_read_at, expected_rate_per_day,
        //   is_primary, is_active, created_at, updated_at
        //
        // New columns:
        //   meter_code  — stable governed identifier (e.g. "HRS-001")
        //   rollover_value — cyclic counter max before reset (NULL = no rollover)
        db.execute_unprepared(
            "ALTER TABLE equipment_meters ADD COLUMN meter_code TEXT",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE equipment_meters ADD COLUMN rollover_value REAL",
        )
        .await?;

        // Backfill meter_code from name for existing rows (idempotent).
        db.execute_unprepared(
            "UPDATE equipment_meters SET meter_code = UPPER(REPLACE(name, ' ', '_')) WHERE meter_code IS NULL",
        )
        .await?;

        // ── 5. Create asset_meter_readings ────────────────────────────────
        //
        // Append-only time-series table. Readings are never updated or
        // deleted — corrected values are inserted with quality_flag = 'corrected'.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_meter_readings"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("meter_id")).integer().not_null())
                    .col(
                        ColumnDef::new(Alias::new("reading_value"))
                            .double()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("reading_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("source_type")).text().not_null())
                    .col(ColumnDef::new(Alias::new("source_reference")).text())
                    .col(
                        ColumnDef::new(Alias::new("quality_flag"))
                            .text()
                            .not_null()
                            .default("accepted"),
                    )
                    .col(ColumnDef::new(Alias::new("created_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── 6. Add meter/reading indexes ──────────────────────────────────
        manager
            .create_index(
                Index::create()
                    .name("idx_meter_readings_meter_at")
                    .table(Alias::new("asset_meter_readings"))
                    .col(Alias::new("meter_id"))
                    .col(Alias::new("reading_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_equipment_meters_asset_primary")
                    .table(Alias::new("equipment_meters"))
                    .col(Alias::new("equipment_id"))
                    .col(Alias::new("is_primary"))
                    .to_owned(),
            )
            .await?;

        // ══════════════════════════════════════════════════════════════════
        // Sprint S3 — Document links
        // ══════════════════════════════════════════════════════════════════

        // ── 7. Create asset_document_links ────────────────────────────────
        //
        // Governed document references. Links are superseded or expired,
        // not hard-deleted. Purpose codes are governed via lookup domain.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_document_links"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("asset_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("document_ref")).text().not_null())
                    .col(ColumnDef::new(Alias::new("link_purpose")).text().not_null())
                    .col(
                        ColumnDef::new(Alias::new("is_primary"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("created_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── 8. Add document link indexes ──────────────────────────────────
        manager
            .create_index(
                Index::create()
                    .name("idx_doc_links_asset_purpose")
                    .table(Alias::new("asset_document_links"))
                    .col(Alias::new("asset_id"))
                    .col(Alias::new("link_purpose"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop Sprint S3 objects first (reverse order)
        manager
            .drop_index(
                Index::drop()
                    .name("idx_doc_links_asset_purpose")
                    .table(Alias::new("asset_document_links"))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_document_links"))
                    .to_owned(),
            )
            .await?;

        // Drop Sprint S2 objects
        manager
            .drop_index(
                Index::drop()
                    .name("idx_equipment_meters_asset_primary")
                    .table(Alias::new("equipment_meters"))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_meter_readings_meter_at")
                    .table(Alias::new("asset_meter_readings"))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_meter_readings"))
                    .to_owned(),
            )
            .await?;

        // SQLite < 3.35 does not support DROP COLUMN.
        // Drop the indexes added in Sprint S1; ALTERed columns remain.
        for idx in [
            "idx_eq_lifecycle_equip_occurred",
            "idx_eq_lifecycle_event_type",
            "idx_eq_lifecycle_related_asset",
        ] {
            manager
                .drop_index(
                    Index::drop()
                        .name(idx)
                        .table(Alias::new("equipment_lifecycle_events"))
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }
}

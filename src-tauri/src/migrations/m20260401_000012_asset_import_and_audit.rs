//! Migration 012 — Asset import staging, batch tracking, and audit events.
//!
//! Phase 2 - Sub-phase 02 - File 04 - Sprint S1.
//!
//! Creates the three tables required for governed bulk import:
//!
//!   `asset_import_batches` — One row per import attempt. Tracks source file
//!   identity (filename + SHA-256), actor, status progression
//!   (uploaded → validated → applied/failed/cancelled), and validation
//!   summary counts (total/valid/warning/error).
//!
//!   `asset_import_staging` — One row per CSV/JSON row within a batch.
//!   Stores the raw input alongside normalized identifiers and per-row
//!   validation outcome (valid/warning/error), conflict classification,
//!   and proposed action (create/update/skip/conflict).
//!
//!   `asset_import_events` — Append-only audit journal for batch lifecycle.
//!   Records upload, validation, apply, cancel, and failure events with
//!   summary payloads and actor attribution.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000012_asset_import_and_audit"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── 1. asset_import_batches ───────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_import_batches"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("source_filename"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("source_sha256"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("initiated_by_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null()
                            .default("uploaded"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("total_rows"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("valid_rows"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("warning_rows"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("error_rows"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("updated_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Index on SHA-256 for idempotency lookups (same file re-upload detection).
        manager
            .create_index(
                Index::create()
                    .name("idx_import_batches_sha256")
                    .table(Alias::new("asset_import_batches"))
                    .col(Alias::new("source_sha256"))
                    .to_owned(),
            )
            .await?;

        // Index on status for filtering active/completed batches.
        manager
            .create_index(
                Index::create()
                    .name("idx_import_batches_status")
                    .table(Alias::new("asset_import_batches"))
                    .col(Alias::new("status"))
                    .to_owned(),
            )
            .await?;

        // ── 2. asset_import_staging ───────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_import_staging"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("batch_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("row_no"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("raw_json"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("normalized_asset_code")).text())
                    .col(ColumnDef::new(Alias::new("normalized_external_key")).text())
                    .col(
                        ColumnDef::new(Alias::new("validation_status"))
                            .text()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("validation_messages_json"))
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(ColumnDef::new(Alias::new("proposed_action")).text())
                    .to_owned(),
            )
            .await?;

        // Composite index: batch + row_no for ordered traversal.
        manager
            .create_index(
                Index::create()
                    .name("idx_import_staging_batch_row")
                    .table(Alias::new("asset_import_staging"))
                    .col(Alias::new("batch_id"))
                    .col(Alias::new("row_no"))
                    .to_owned(),
            )
            .await?;

        // Index on normalized_asset_code for duplicate detection within batch.
        manager
            .create_index(
                Index::create()
                    .name("idx_import_staging_asset_code")
                    .table(Alias::new("asset_import_staging"))
                    .col(Alias::new("normalized_asset_code"))
                    .to_owned(),
            )
            .await?;

        // Index on validation_status for count queries.
        manager
            .create_index(
                Index::create()
                    .name("idx_import_staging_validation")
                    .table(Alias::new("asset_import_staging"))
                    .col(Alias::new("batch_id"))
                    .col(Alias::new("validation_status"))
                    .to_owned(),
            )
            .await?;

        // ── 3. asset_import_events ────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("asset_import_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("batch_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("event_type"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("summary_json")).text())
                    .col(ColumnDef::new(Alias::new("created_by_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Composite index: batch + event_type for filtered event retrieval.
        manager
            .create_index(
                Index::create()
                    .name("idx_import_events_batch_type")
                    .table(Alias::new("asset_import_events"))
                    .col(Alias::new("batch_id"))
                    .col(Alias::new("event_type"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Reverse order: events → staging → batches.
        for idx in [
            "idx_import_events_batch_type",
        ] {
            manager
                .drop_index(
                    Index::drop()
                        .name(idx)
                        .table(Alias::new("asset_import_events"))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_import_events"))
                    .to_owned(),
            )
            .await?;

        for idx in [
            "idx_import_staging_validation",
            "idx_import_staging_asset_code",
            "idx_import_staging_batch_row",
        ] {
            manager
                .drop_index(
                    Index::drop()
                        .name(idx)
                        .table(Alias::new("asset_import_staging"))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_import_staging"))
                    .to_owned(),
            )
            .await?;

        for idx in [
            "idx_import_batches_status",
            "idx_import_batches_sha256",
        ] {
            manager
                .drop_index(
                    Index::drop()
                        .name(idx)
                        .table(Alias::new("asset_import_batches"))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("asset_import_batches"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

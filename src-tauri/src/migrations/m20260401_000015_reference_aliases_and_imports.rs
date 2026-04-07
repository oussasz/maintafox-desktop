//! Migration 015 — Reference aliases and import batch tables.
//!
//! Phase 2 - Sub-phase 03 - File 03 - Sprints S1 + S2.
//!
//! Creates:
//!   - `reference_aliases`: typed, locale-aware alias records for reference values.
//!     Supports legacy/import/search alias types with at most one preferred alias
//!     per `(reference_value_id, locale, alias_type)`.
//!   - `reference_import_batches`: staged import lifecycle (uploaded → validated → applied/failed).
//!   - `reference_import_rows`: per-row staging with validation diagnostics.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000015_reference_aliases_and_imports"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── reference_aliases ─────────────────────────────────────────────────
        // Typed, locale-aware alias records for reference values. Each alias binds
        // to a specific reference_value_id and carries a locale, an alias_type
        // (legacy | import | search), and a preferred flag. The preferred flag is
        // constrained to at most one per (value, locale, type) in application logic
        // since SQLite partial unique indexes are limited.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_aliases"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // FK to reference_values.id — the canonical value this alias maps to.
                    .col(
                        ColumnDef::new(Alias::new("reference_value_id"))
                            .integer()
                            .not_null(),
                    )
                    // The alias text itself.
                    .col(
                        ColumnDef::new(Alias::new("alias_label"))
                            .text()
                            .not_null(),
                    )
                    // BCP-47 locale tag (e.g. "fr", "en").
                    .col(
                        ColumnDef::new(Alias::new("locale"))
                            .text()
                            .not_null(),
                    )
                    // Alias classification: legacy | import | search.
                    .col(
                        ColumnDef::new(Alias::new("alias_type"))
                            .text()
                            .not_null(),
                    )
                    // At most one preferred alias per (value, locale, type).
                    // Enforced in application logic, stored as 0/1.
                    .col(
                        ColumnDef::new(Alias::new("is_preferred"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Lookup index: "all aliases for a given reference value".
        manager
            .create_index(
                Index::create()
                    .name("idx_reference_aliases_value")
                    .table(Alias::new("reference_aliases"))
                    .col(Alias::new("reference_value_id"))
                    .to_owned(),
            )
            .await?;

        // Uniqueness guard: same alias label cannot appear twice for the same
        // (value, locale, type) combination.
        manager
            .create_index(
                Index::create()
                    .name("idx_reference_aliases_unique_label")
                    .table(Alias::new("reference_aliases"))
                    .col(Alias::new("reference_value_id"))
                    .col(Alias::new("locale"))
                    .col(Alias::new("alias_type"))
                    .col(Alias::new("alias_label"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // ── reference_import_batches ──────────────────────────────────────────
        // Tracks each import session through its lifecycle:
        //   uploaded → validated → applied | failed
        // Batch identity includes a SHA-256 hash of the source file for
        // deduplication and traceability.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_import_batches"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // The reference domain this import targets.
                    .col(
                        ColumnDef::new(Alias::new("domain_id"))
                            .integer()
                            .not_null(),
                    )
                    // Original file name for audit display.
                    .col(
                        ColumnDef::new(Alias::new("source_filename"))
                            .text()
                            .not_null(),
                    )
                    // SHA-256 digest of the source file content.
                    .col(
                        ColumnDef::new(Alias::new("source_sha256"))
                            .text()
                            .not_null(),
                    )
                    // Lifecycle state: uploaded | validated | applied | failed
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null(),
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
                    // User who initiated the import (null for system-initiated).
                    .col(ColumnDef::new(Alias::new("initiated_by_id")).integer())
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

        // Domain-scoped batch listing.
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_import_batches_domain")
                    .table(Alias::new("reference_import_batches"))
                    .col(Alias::new("domain_id"))
                    .to_owned(),
            )
            .await?;

        // ── reference_import_rows ─────────────────────────────────────────────
        // Staging rows for a batch. Each row carries the raw JSON payload,
        // a normalized code for resolution, per-row validation status, and
        // structured diagnostic messages.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_import_rows"))
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
                    // 1-based row number from the source file.
                    .col(
                        ColumnDef::new(Alias::new("row_no"))
                            .integer()
                            .not_null(),
                    )
                    // Original row data as JSON object.
                    .col(
                        ColumnDef::new(Alias::new("raw_json"))
                            .text()
                            .not_null(),
                    )
                    // Normalized reference value code extracted from the row.
                    .col(ColumnDef::new(Alias::new("normalized_code")).text())
                    // Row validation outcome: pending | valid | warning | error
                    .col(
                        ColumnDef::new(Alias::new("validation_status"))
                            .text()
                            .not_null(),
                    )
                    // JSON array of diagnostic messages.
                    .col(
                        ColumnDef::new(Alias::new("messages_json"))
                            .text()
                            .not_null(),
                    )
                    // Resolution action: create | update | skip | null
                    .col(ColumnDef::new(Alias::new("proposed_action")).text())
                    .to_owned(),
            )
            .await?;

        // Batch-scoped row listing.
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_import_rows_batch")
                    .table(Alias::new("reference_import_rows"))
                    .col(Alias::new("batch_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("reference_import_rows")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("reference_import_batches")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("reference_aliases")).to_owned())
            .await?;
        Ok(())
    }
}

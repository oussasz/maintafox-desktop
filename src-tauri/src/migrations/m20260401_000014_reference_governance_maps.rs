//! Migration 014 — Reference governance maps.
//!
//! Phase 2 - Sub-phase 03 - File 02 - Sprint S1.
//!
//! Creates:
//!   - `reference_value_migrations`: tracks merge and migration mappings so
//!     historical traceability is retained when values are replaced or consolidated.
//!   - `reference_validation_reports`: persists structured validation diagnostics
//!     for each set validation run, enabling audit of publish-readiness decisions.
//!
//! These tables support the protected-domain policy layer and the validation
//! workflow (Sprints S1–S2 of File 02).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000014_reference_governance_maps"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── reference_value_migrations ─────────────────────────────────────────
        // Records every merge or migration of a reference value to another.
        // When an in-use value is consolidated, the mapping row preserves the
        // traceability chain so historical records that referenced the old code
        // can still be interpreted.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_value_migrations"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // The domain in which the migration occurred.
                    .col(
                        ColumnDef::new(Alias::new("domain_id"))
                            .integer()
                            .not_null(),
                    )
                    // The value being retired or merged away.
                    .col(
                        ColumnDef::new(Alias::new("from_value_id"))
                            .integer()
                            .not_null(),
                    )
                    // The surviving target value.
                    .col(
                        ColumnDef::new(Alias::new("to_value_id"))
                            .integer()
                            .not_null(),
                    )
                    // Optional coded reason for the migration (e.g. "DUPLICATE", "CONSOLIDATION").
                    .col(ColumnDef::new(Alias::new("reason_code")).text())
                    // User who performed the migration (nullable for system-initiated).
                    .col(ColumnDef::new(Alias::new("migrated_by_id")).integer())
                    // ISO 8601 timestamp when the migration was recorded.
                    .col(
                        ColumnDef::new(Alias::new("migrated_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Domain-scoped lookups: "show all migrations for this domain".
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_value_migrations_domain")
                    .table(Alias::new("reference_value_migrations"))
                    .col(Alias::new("domain_id"))
                    .to_owned(),
            )
            .await?;

        // Reverse lookup: "what was this value migrated from?"
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_value_migrations_from")
                    .table(Alias::new("reference_value_migrations"))
                    .col(Alias::new("from_value_id"))
                    .to_owned(),
            )
            .await?;

        // Forward lookup: "what was migrated TO this value?"
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_value_migrations_to")
                    .table(Alias::new("reference_value_migrations"))
                    .col(Alias::new("to_value_id"))
                    .to_owned(),
            )
            .await?;

        // ── reference_validation_reports ───────────────────────────────────────
        // Persists the result of each set validation run. The full structured
        // issue list is stored in report_json so validation decisions are auditable.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("reference_validation_reports"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // The set that was validated.
                    .col(
                        ColumnDef::new(Alias::new("set_id"))
                            .integer()
                            .not_null(),
                    )
                    // "passed" | "failed"
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null(),
                    )
                    // Total number of issues found (blocking + non-blocking).
                    .col(
                        ColumnDef::new(Alias::new("issue_count"))
                            .integer()
                            .not_null(),
                    )
                    // Number of blocking issues that prevent publish.
                    .col(
                        ColumnDef::new(Alias::new("blocking_count"))
                            .integer()
                            .not_null(),
                    )
                    // Full structured issue list as JSON array.
                    .col(
                        ColumnDef::new(Alias::new("report_json"))
                            .text()
                            .not_null(),
                    )
                    // User who triggered the validation (nullable for system-initiated).
                    .col(ColumnDef::new(Alias::new("validated_by_id")).integer())
                    // ISO 8601 timestamp of the validation run.
                    .col(
                        ColumnDef::new(Alias::new("validated_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Set-scoped lookups: "show all validation reports for this set".
        manager
            .create_index(
                Index::create()
                    .name("idx_ref_validation_reports_set")
                    .table(Alias::new("reference_validation_reports"))
                    .col(Alias::new("set_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("reference_validation_reports"))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("reference_value_migrations"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

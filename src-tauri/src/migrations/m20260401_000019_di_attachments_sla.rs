//! Migration 019 — DI attachments, SLA rules, and WO stubs.
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S1.
//!
//! Creates:
//!   - `di_attachments`: files (photos, sensor snapshots, PDFs) linked to a DI
//!     at any point in its lifecycle. Relative paths only; Tauri resolves at runtime.
//!   - `di_sla_rules`: configurable SLA targets by urgency, origin, and asset criticality.
//!     Seeded with sensible defaults for the four urgency levels.
//!   - `work_order_stubs`: minimal WO shell for DI-to-WO conversion traceability.
//!     SP05 will replace this with the full `work_orders` table.
//!
//! Foreign key dependencies:
//!   - `intervention_requests` (migration 017)
//!   - `user_accounts` (migration 002)

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000019_di_attachments_sla"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── di_attachments ────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("di_attachments"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("di_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("file_name"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("relative_path"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("mime_type"))
                            .text()
                            .not_null()
                            .default("application/octet-stream"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("size_bytes"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("attachment_type"))
                            .text()
                            .not_null()
                            .default("other"),
                    )
                    .col(ColumnDef::new(Alias::new("uploaded_by_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("uploaded_at"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .to_owned(),
            )
            .await?;

        // -- Unique constraint on relative_path --
        manager
            .create_index(
                Index::create()
                    .name("idx_da_relative_path_unique")
                    .table(Alias::new("di_attachments"))
                    .col(Alias::new("relative_path"))
                    .unique()
                    .to_owned(),
            )
            .await?;

        // -- Index on di_id for fast lookups --
        manager
            .create_index(
                Index::create()
                    .name("idx_da_di_id")
                    .table(Alias::new("di_attachments"))
                    .col(Alias::new("di_id"))
                    .to_owned(),
            )
            .await?;

        // ── di_sla_rules ──────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("di_sla_rules"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("name"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("urgency_level"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("origin_type")).text())
                    .col(ColumnDef::new(Alias::new("asset_criticality_class")).text())
                    .col(
                        ColumnDef::new(Alias::new("target_response_hours"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("target_resolution_hours"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("escalation_threshold_hours"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // ── Seed default SLA rules ────────────────────────────────────────
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO di_sla_rules \
                (name, urgency_level, target_response_hours, target_resolution_hours, escalation_threshold_hours) \
             VALUES \
                ('Critical - All Origins', 'critical', 1, 8, 4), \
                ('High - All Origins', 'high', 4, 24, 8), \
                ('Medium - All Origins', 'medium', 24, 72, 48), \
                ('Low - All Origins', 'low', 72, 168, 120)"
                .to_string(),
        ))
        .await?;

        // ── work_order_stubs (minimal shell for DI→WO conversion) ─────────
        // SP05 will replace this with the full `work_orders` table.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("work_order_stubs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("code"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Alias::new("source_di_id")).integer())
                    .col(ColumnDef::new(Alias::new("asset_id")).integer())
                    .col(ColumnDef::new(Alias::new("org_node_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("title"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("urgency"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null()
                            .default("draft"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("work_order_stubs")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("di_sla_rules")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("di_attachments")).to_owned())
            .await?;
        Ok(())
    }
}

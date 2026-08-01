//! Migration 026 — WO change events (append-only audit ledger) + ot.* permission domain seed.
//!
//! Phase 2 - Sub-phase 05 - File 04 - Sprint S1.
//!
//! Creates:
//!   - `wo_change_events`: immutable audit trail for every WO lifecycle action.
//!     No UPDATE or DELETE operations exist for this table.
//!     Blocked dangerous actions are also recorded (apply_result = 'blocked').
//!
//! Seeds:
//!   - 8 `ot.*` permissions in the `permissions` table, aligned with the canonical
//!     domain defined in PRD §6.7.
//!
//! Foreign key dependencies:
//!   - `work_orders` (migration 022)
//!   - `user_accounts` (migration 002)
//!   - `permissions` table (migration 001)

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260411_000026_wo_change_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── wo_change_events — append-only audit ledger ───────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("wo_change_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("wo_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("action"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("actor_id")).integer())
                    .col(
                        ColumnDef::new(Alias::new("acted_at"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("summary")).text())
                    .col(ColumnDef::new(Alias::new("details_json")).text())
                    .col(
                        ColumnDef::new(Alias::new("requires_step_up"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("apply_result"))
                            .text()
                            .not_null()
                            .default("applied"),
                    )
                    .to_owned(),
            )
            .await?;

        // ── Indexes for query performance ─────────────────────────────────
        manager
            .create_index(
                Index::create()
                    .name("idx_wce_wo_id")
                    .table(Alias::new("wo_change_events"))
                    .col(Alias::new("wo_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_wce_action")
                    .table(Alias::new("wo_change_events"))
                    .col(Alias::new("action"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_wce_actor")
                    .table(Alias::new("wo_change_events"))
                    .col(Alias::new("actor_id"))
                    .to_owned(),
            )
            .await?;

        // ── Seed canonical ot.* permission domain ─────────────────────────
        // Uses INSERT OR IGNORE to coexist with any pre-existing seeder rows.
        // PRD §6.7 defines 8 ot.* permissions for the Work Order domain.
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permissions
                   (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES
                   ('ot.view',    'View work orders and details',                 'ot', 0, 0, 1, ?),
                   ('ot.create',  'Create new work orders',                      'ot', 0, 0, 1, ?),
                   ('ot.edit',    'Edit, plan, assign, and execute work orders', 'ot', 0, 0, 1, ?),
                   ('ot.approve', 'Approve work orders from draft',              'ot', 0, 0, 1, ?),
                   ('ot.close',   'Close technically verified work orders',      'ot', 1, 1, 1, ?),
                   ('ot.reopen',  'Reopen recently closed work orders',          'ot', 1, 1, 1, ?),
                   ('ot.admin',   'Override, archive, manage WO settings',       'ot', 1, 0, 1, ?),
                   ('ot.delete',  'Delete draft work orders',                    'ot', 1, 0, 1, ?)",
            [
                now.clone().into(), now.clone().into(), now.clone().into(), now.clone().into(),
                now.clone().into(), now.clone().into(), now.clone().into(), now.into(),
            ],
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("wo_change_events")).to_owned())
            .await?;

        // Remove the 8 ot.* permissions seeded by this migration
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM role_permissions WHERE permission_id IN \
             (SELECT id FROM permissions WHERE name LIKE 'ot.%')"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM permissions WHERE name LIKE 'ot.%'".to_string(),
        ))
        .await?;

        Ok(())
    }
}

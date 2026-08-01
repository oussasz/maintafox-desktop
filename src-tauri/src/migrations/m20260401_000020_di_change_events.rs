//! Migration 020 — DI change events (append-only audit ledger) + DI permission domain seed.
//!
//! Phase 2 - Sub-phase 04 - File 04 - Sprint S1.
//!
//! Creates:
//!   - `di_change_events`: immutable audit trail for every DI lifecycle action.
//!     No UPDATE or DELETE operations exist for this table.
//!     Blocked dangerous actions are also recorded (apply_result = 'blocked').
//!
//! Seeds:
//!   - 7 `di.*` permissions in the `permissions` table, reconciling the stale
//!     entries from the initial seeder with the canonical domain defined in PRD §6.7.
//!
//! Foreign key dependencies:
//!   - `intervention_requests` (migration 017)
//!   - `user_accounts` (migration 002)
//!   - `permissions` table (migration 001)

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000020_di_change_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── di_change_events — append-only audit ledger ───────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("di_change_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Alias::new("di_id")).integer())
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
                    .name("idx_dce_di_id")
                    .table(Alias::new("di_change_events"))
                    .col(Alias::new("di_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dce_action")
                    .table(Alias::new("di_change_events"))
                    .col(Alias::new("action"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dce_actor")
                    .table(Alias::new("di_change_events"))
                    .col(Alias::new("actor_id"))
                    .to_owned(),
            )
            .await?;

        // ── Seed canonical di.* permission domain ─────────────────────────
        // Uses INSERT OR IGNORE to coexist with the seeder's existing rows.
        // The seeder seeds di.view, di.create, di.edit, di.delete, di.review,
        // di.approve, di.close. This migration adds the 3 missing permissions
        // that commands actually reference: di.create.own, di.convert, di.admin.
        let db = manager.get_connection();
        let now = chrono::Utc::now().to_rfc3339();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT OR IGNORE INTO permissions
                   (name, description, category, is_dangerous, requires_step_up, is_system, created_at)
               VALUES
                   ('di.create.own', 'Submit intervention requests (own entity only)', 'intervention', 0, 0, 1, ?),
                   ('di.convert', 'Convert approved DI to work order', 'intervention', 1, 1, 1, ?),
                   ('di.admin', 'Override, archive, reopen, manage SLA rules', 'intervention', 1, 0, 1, ?)",
            [now.clone().into(), now.clone().into(), now.into()],
        ))
        .await?;

        // ── Update di.approve to mark as dangerous (was not flagged in original seeder) ──
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE permissions SET is_dangerous = 1 WHERE name = 'di.approve' AND is_dangerous = 0"
                .to_string(),
        ))
        .await?;

        // ── Remove legacy di.* permissions that no command references ─────
        // di.edit, di.delete, di.close were placeholders from the initial seeder.
        // The canonical domain per PRD §6.7 is exactly 7 permissions.
        // Clean up role_permissions first (FK constraint), then the permission rows.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM role_permissions WHERE permission_id IN \
             (SELECT id FROM permissions WHERE name IN ('di.edit', 'di.delete', 'di.close'))"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM permissions WHERE name IN ('di.edit', 'di.delete', 'di.close')"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("di_change_events")).to_owned())
            .await?;

        // Remove the 3 permissions added by this migration (leave seeder-owned rows)
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DELETE FROM permissions WHERE name IN ('di.create.own', 'di.convert', 'di.admin')"
                .to_string(),
        ))
        .await?;

        Ok(())
    }
}

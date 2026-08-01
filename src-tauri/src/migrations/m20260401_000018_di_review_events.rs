//! Migration 018 — DI review events (append-only triage decision log).
//!
//! Phase 2 - Sub-phase 04 - File 02 - Sprint S1.
//!
//! Creates:
//!   - `di_review_events`: append-only table recording every triage decision
//!     (screen, return, reject, approve, defer, reactivate) with actor,
//!     timestamp, reasoning, and SLA context.
//!
//! Foreign key dependencies:
//!   - `intervention_requests` (migration 017)
//!   - `user_accounts` (migration 002)

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000018_di_review_events"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── di_review_events (append-only) ────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("di_review_events"))
                    .if_not_exists()
                    // -- PK --
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // -- FK to intervention_requests --
                    .col(
                        ColumnDef::new(Alias::new("di_id"))
                            .integer()
                            .not_null(),
                    )
                    // -- Event classification --
                    // submitted | screened | advanced_to_approval |
                    // returned_for_clarification | rejected | approved |
                    // deferred | reactivated | sla_initialized
                    .col(
                        ColumnDef::new(Alias::new("event_type"))
                            .text()
                            .not_null(),
                    )
                    // -- Actor (nullable for system-generated events) --
                    .col(ColumnDef::new(Alias::new("actor_id")).integer())
                    // -- Timestamp --
                    .col(
                        ColumnDef::new(Alias::new("acted_at"))
                            .text()
                            .not_null(),
                    )
                    // -- Decision data --
                    .col(
                        ColumnDef::new(Alias::new("from_status"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("to_status"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("reason_code")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    // -- SLA context (set on screen action) --
                    .col(ColumnDef::new(Alias::new("sla_target_hours")).integer())
                    .col(ColumnDef::new(Alias::new("sla_deadline")).text())
                    // -- Step-up used flag --
                    .col(
                        ColumnDef::new(Alias::new("step_up_used"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // ── Indexes ───────────────────────────────────────────────────────
        manager
            .create_index(
                Index::create()
                    .name("idx_dre_di_id")
                    .table(Alias::new("di_review_events"))
                    .col(Alias::new("di_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dre_actor")
                    .table(Alias::new("di_review_events"))
                    .col(Alias::new("actor_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dre_type")
                    .table(Alias::new("di_review_events"))
                    .col(Alias::new("event_type"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("di_review_events"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

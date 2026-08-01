//! Migration 017 — Intervention request (DI) domain core.
//!
//! Phase 2 - Sub-phase 04 - File 01 - Sprint S1.
//!
//! Creates:
//!   - `intervention_requests`: the formal demand intake table for all reactive and
//!     semi-reactive maintenance. Carries triage evidence, impact flags, urgency,
//!     SLA timestamps, review/approval tracking, and WO conversion linkage.
//!   - `di_state_transition_log`: append-only audit trail for every state movement
//!     in the 11-state PRD §6.4 workflow.
//!
//! Foreign key dependencies:
//!   - `asset_registry` (migration 010 / equipment 005)
//!   - `org_nodes` (migration 004)
//!   - `user_accounts` (migration 002)
//!   - `reference_values` (migration 013) — nullable for symptom/classification codes

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000017_di_domain_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── intervention_requests ─────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("intervention_requests"))
                    .if_not_exists()
                    // -- PK --
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // -- Code (DI-0001 format, unique, non-recycled) --
                    .col(
                        ColumnDef::new(Alias::new("code"))
                            .text()
                            .not_null()
                            .unique_key(),
                    )
                    // -- Origin context --
                    .col(
                        ColumnDef::new(Alias::new("asset_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("sub_asset_ref")).text())
                    .col(
                        ColumnDef::new(Alias::new("org_node_id"))
                            .integer()
                            .not_null(),
                    )
                    // -- State (PRD §6.4 11-state machine) --
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .text()
                            .not_null()
                            .default("submitted"),
                    )
                    // -- Triage evidence --
                    .col(
                        ColumnDef::new(Alias::new("title"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("description"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("origin_type"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("symptom_code_id")).integer())
                    // -- Impact flags --
                    .col(
                        ColumnDef::new(Alias::new("impact_level"))
                            .text()
                            .not_null()
                            .default("unknown"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("production_impact"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("safety_flag"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("environmental_flag"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("quality_flag"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // -- Priority --
                    .col(
                        ColumnDef::new(Alias::new("reported_urgency"))
                            .text()
                            .not_null()
                            .default("medium"),
                    )
                    .col(ColumnDef::new(Alias::new("validated_urgency")).text())
                    // -- Timing (SLA origin) --
                    .col(ColumnDef::new(Alias::new("observed_at")).text())
                    .col(
                        ColumnDef::new(Alias::new("submitted_at"))
                            .text()
                            .not_null(),
                    )
                    // -- Review / approval tracking --
                    .col(ColumnDef::new(Alias::new("review_team_id")).integer())
                    .col(ColumnDef::new(Alias::new("reviewer_id")).integer())
                    .col(ColumnDef::new(Alias::new("screened_at")).text())
                    .col(ColumnDef::new(Alias::new("approved_at")).text())
                    .col(ColumnDef::new(Alias::new("deferred_until")).text())
                    .col(ColumnDef::new(Alias::new("declined_at")).text())
                    .col(ColumnDef::new(Alias::new("closed_at")).text())
                    .col(ColumnDef::new(Alias::new("archived_at")).text())
                    // -- WO linkage (nullable until SP05) --
                    .col(ColumnDef::new(Alias::new("converted_to_wo_id")).integer())
                    .col(ColumnDef::new(Alias::new("converted_at")).text())
                    // -- Review decision fields --
                    .col(ColumnDef::new(Alias::new("reviewer_note")).text())
                    .col(
                        ColumnDef::new(Alias::new("classification_code_id")).integer(),
                    )
                    // -- Recurrence --
                    .col(
                        ColumnDef::new(Alias::new("is_recurrence_flag"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Alias::new("recurrence_di_id")).integer())
                    // -- Concurrency --
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // -- Metadata --
                    .col(
                        ColumnDef::new(Alias::new("submitter_id"))
                            .integer()
                            .not_null(),
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

        // -- Indexes for intervention_requests --
        manager
            .create_index(
                Index::create()
                    .name("idx_ir_status")
                    .table(Alias::new("intervention_requests"))
                    .col(Alias::new("status"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ir_asset")
                    .table(Alias::new("intervention_requests"))
                    .col(Alias::new("asset_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ir_org_node")
                    .table(Alias::new("intervention_requests"))
                    .col(Alias::new("org_node_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ir_submitter")
                    .table(Alias::new("intervention_requests"))
                    .col(Alias::new("submitter_id"))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_ir_reviewer")
                    .table(Alias::new("intervention_requests"))
                    .col(Alias::new("reviewer_id"))
                    .to_owned(),
            )
            .await?;

        // ── di_state_transition_log (append-only) ─────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("di_state_transition_log"))
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
                        ColumnDef::new(Alias::new("from_status"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("to_status"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("action"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("actor_id")).integer())
                    .col(ColumnDef::new(Alias::new("reason_code")).text())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(
                        ColumnDef::new(Alias::new("acted_at"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_dstl_di_id")
                    .table(Alias::new("di_state_transition_log"))
                    .col(Alias::new("di_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("di_state_transition_log"))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(Alias::new("intervention_requests"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

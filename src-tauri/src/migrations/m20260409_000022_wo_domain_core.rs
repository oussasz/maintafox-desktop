//! Migration 022 — WO domain core tables.
//!
//! Phase 2 - Sub-phase 05 - File 01 - Sprint S1.
//!
//! Creates:
//!   - `work_order_types`: 7 PRD-seeded system types (corrective, preventive, etc.)
//!   - `work_order_statuses`: 12-state PRD §6.5 workflow with macro_state and terminal flags
//!   - `urgency_levels`: fixed 5-level scale (1=Very Low → 5=Critical)
//!   - `delay_reason_codes`: 10 standard delay categories
//!   - `work_orders`: full WO table superseding `work_order_stubs` from migration 019
//!   - `wo_state_transition_log`: append-only state change audit ledger
//!
//! Data migration:
//!   Existing `work_order_stubs` rows are migrated into `work_orders` before the
//!   stub table is dropped. Stub columns map as:
//!     code → code, source_di_id → source_di_id, asset_id → equipment_id,
//!     org_node_id → location_id, title → title, urgency → urgency_id (label-matched),
//!     status → always 'draft', type → always 'corrective'.

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260409_000022_wo_domain_core"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── work_order_types ──────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("work_order_types"))
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
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_system"))
                            .integer()
                            .not_null()
                            .default(0),
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

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO work_order_types (code, label, is_system) VALUES \
                ('corrective',      'Corrective',       1), \
                ('preventive',      'Preventive',       1), \
                ('improvement',     'Improvement',      1), \
                ('inspection',      'Inspection',       1), \
                ('emergency',       'Emergency',        1), \
                ('overhaul',        'Overhaul',         1), \
                ('condition_based', 'Condition-Based',  1)"
                .to_string(),
        ))
        .await?;

        // ── work_order_statuses ───────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("work_order_statuses"))
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
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("color"))
                            .text()
                            .not_null()
                            .default("#808080"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("macro_state"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_terminal"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_system"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("sequence"))
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO work_order_statuses (code, label, color, macro_state, is_terminal, is_system, sequence) VALUES \
                ('draft',                    'Draft',                    '#94A3B8', 'open',      0, 1, 1), \
                ('awaiting_approval',        'Awaiting Approval',        '#F59E0B', 'open',      0, 1, 2), \
                ('planned',                  'Planned',                  '#3B82F6', 'open',      0, 1, 3), \
                ('ready_to_schedule',        'Ready To Schedule',        '#6366F1', 'open',      0, 1, 4), \
                ('assigned',                 'Assigned',                 '#8B5CF6', 'executing', 0, 1, 5), \
                ('waiting_for_prerequisite', 'Waiting For Prerequisite', '#F97316', 'executing', 0, 1, 6), \
                ('in_progress',              'In Progress',              '#10B981', 'executing', 0, 1, 7), \
                ('paused',                   'Paused',                   '#EF4444', 'executing', 0, 1, 8), \
                ('mechanically_complete',    'Mechanically Complete',    '#06B6D4', 'completed', 0, 1, 9), \
                ('technically_verified',     'Technically Verified',     '#22C55E', 'completed', 0, 1, 10), \
                ('closed',                   'Closed',                   '#64748B', 'closed',    1, 1, 11), \
                ('cancelled',                'Cancelled',                '#DC2626', 'cancelled', 1, 1, 12)"
                .to_string(),
        ))
        .await?;

        // ── urgency_levels ────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("urgency_levels"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("level"))
                            .integer()
                            .not_null()
                            .unique_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("label_fr"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("hex_color"))
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO urgency_levels (level, label, label_fr, hex_color) VALUES \
                (1, 'Very Low',  'Tres Faible', '#64748B'), \
                (2, 'Low',       'Faible',      '#3B82F6'), \
                (3, 'Medium',    'Moyenne',     '#F59E0B'), \
                (4, 'High',      'Haute',       '#F97316'), \
                (5, 'Critical',  'Critique',    '#DC2626')"
                .to_string(),
        ))
        .await?;

        // ── delay_reason_codes ────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("delay_reason_codes"))
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
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("category"))
                            .text()
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

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO delay_reason_codes (code, label, category) VALUES \
                ('no_parts',       'Awaiting Spare Parts',       'parts'), \
                ('backordered',    'Parts Backordered',          'parts'), \
                ('no_permit',      'Permit Not Issued',          'permit'), \
                ('permit_expired', 'Permit Expired / Revoked',   'permit'), \
                ('no_shutdown',    'Shutdown Window Unavailable', 'shutdown'), \
                ('vendor_delay',   'Vendor / Contractor Delay',  'vendor'), \
                ('no_labor',       'Insufficient Labor',         'labor'), \
                ('no_access',      'Access to Equipment Denied', 'access'), \
                ('diagnosis',      'Awaiting Diagnosis Result',  'diagnosis'), \
                ('other',          'Other (see notes)',          'other')"
                .to_string(),
        ))
        .await?;

        // ── work_orders (full table, supersedes work_order_stubs) ─────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("work_orders"))
                    .if_not_exists()
                    // Identity
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
                    // Classification
                    .col(
                        ColumnDef::new(Alias::new("type_id"))
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("status_id"))
                            .integer()
                            .not_null(),
                    )
                    // Asset context
                    .col(ColumnDef::new(Alias::new("equipment_id")).integer())
                    .col(ColumnDef::new(Alias::new("component_id")).integer())
                    .col(ColumnDef::new(Alias::new("location_id")).integer())
                    // People
                    .col(ColumnDef::new(Alias::new("requester_id")).integer())
                    .col(ColumnDef::new(Alias::new("source_di_id")).integer())
                    .col(ColumnDef::new(Alias::new("entity_id")).integer())
                    .col(ColumnDef::new(Alias::new("planner_id")).integer())
                    .col(ColumnDef::new(Alias::new("approver_id")).integer())
                    .col(ColumnDef::new(Alias::new("assigned_group_id")).integer())
                    .col(ColumnDef::new(Alias::new("primary_responsible_id")).integer())
                    // Urgency
                    .col(ColumnDef::new(Alias::new("urgency_id")).integer())
                    // Core description
                    .col(
                        ColumnDef::new(Alias::new("title"))
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // Timing
                    .col(ColumnDef::new(Alias::new("planned_start")).text())
                    .col(ColumnDef::new(Alias::new("planned_end")).text())
                    .col(ColumnDef::new(Alias::new("scheduled_at")).text())
                    .col(ColumnDef::new(Alias::new("actual_start")).text())
                    .col(ColumnDef::new(Alias::new("actual_end")).text())
                    .col(ColumnDef::new(Alias::new("mechanically_completed_at")).text())
                    .col(ColumnDef::new(Alias::new("technically_verified_at")).text())
                    .col(ColumnDef::new(Alias::new("closed_at")).text())
                    .col(ColumnDef::new(Alias::new("cancelled_at")).text())
                    // Duration accumulators
                    .col(ColumnDef::new(Alias::new("expected_duration_hours")).double())
                    .col(ColumnDef::new(Alias::new("actual_duration_hours")).double())
                    .col(
                        ColumnDef::new(Alias::new("active_labor_hours"))
                            .double()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("total_waiting_hours"))
                            .double()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("downtime_hours"))
                            .double()
                            .default(0),
                    )
                    // Cost accumulators
                    .col(
                        ColumnDef::new(Alias::new("labor_cost"))
                            .double()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("parts_cost"))
                            .double()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("service_cost"))
                            .double()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Alias::new("total_cost"))
                            .double()
                            .default(0),
                    )
                    // Close-out evidence
                    .col(ColumnDef::new(Alias::new("recurrence_risk_level")).text())
                    .col(ColumnDef::new(Alias::new("production_impact_id")).integer())
                    .col(ColumnDef::new(Alias::new("root_cause_summary")).text())
                    .col(ColumnDef::new(Alias::new("corrective_action_summary")).text())
                    .col(ColumnDef::new(Alias::new("verification_method")).text())
                    // Metadata
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("cancel_reason")).text())
                    .col(
                        ColumnDef::new(Alias::new("row_version"))
                            .integer()
                            .not_null()
                            .default(1),
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

        // Indexes on work_orders
        for (name, col) in [
            ("idx_wo_status", "status_id"),
            ("idx_wo_equipment", "equipment_id"),
            ("idx_wo_entity", "entity_id"),
            ("idx_wo_planner", "planner_id"),
            ("idx_wo_source_di", "source_di_id"),
            ("idx_wo_urgency", "urgency_id"),
        ] {
            manager
                .create_index(
                    Index::create()
                        .name(name)
                        .table(Alias::new("work_orders"))
                        .col(Alias::new(col))
                        .to_owned(),
                )
                .await?;
        }

        // ── Migrate work_order_stubs data ─────────────────────────────────
        // The stub table may not exist if migration 019 was never run on this
        // database (theoretical edge case). Use a conditional check.
        let has_stubs = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='work_order_stubs'"
                    .to_string(),
            ))
            .await?;

        if has_stubs.is_some() {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO work_orders \
                    (code, source_di_id, equipment_id, location_id, title, urgency_id, \
                     status_id, type_id, created_at, updated_at) \
                 SELECT \
                    s.code, \
                    s.source_di_id, \
                    s.asset_id, \
                    s.org_node_id, \
                    s.title, \
                    (SELECT id FROM urgency_levels WHERE label = s.urgency LIMIT 1), \
                    (SELECT id FROM work_order_statuses WHERE code = 'draft' LIMIT 1), \
                    (SELECT id FROM work_order_types WHERE code = 'corrective' LIMIT 1), \
                    s.created_at, \
                    s.created_at \
                 FROM work_order_stubs s \
                 WHERE NOT EXISTS (SELECT 1 FROM work_orders w WHERE w.code = s.code)"
                    .to_string(),
            ))
            .await?;

            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "DROP TABLE work_order_stubs".to_string(),
            ))
            .await?;
        }

        // ── wo_state_transition_log (append-only) ─────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("wo_state_transition_log"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("wo_id"))
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
                    .name("idx_wostl_wo_id")
                    .table(Alias::new("wo_state_transition_log"))
                    .col(Alias::new("wo_id"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Alias::new("wo_state_transition_log")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("work_orders")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("delay_reason_codes")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("urgency_levels")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("work_order_statuses")).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Alias::new("work_order_types")).to_owned())
            .await?;
        Ok(())
    }
}

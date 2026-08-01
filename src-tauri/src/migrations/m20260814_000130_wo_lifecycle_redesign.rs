//! Migration 130 — WO lifecycle redesign (Option B).
//!
//! Replaces the 12-status catalog with:
//!   draft → planning → ready → in_progress ↔ on_hold → completed → closed
//!   (+ cancelled)
//! Remaps existing work_orders.status_id. Adds wo_action_events and
//! planning_approved_at for readiness approval rule.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260814_000130_wo_lifecycle_redesign"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── New status rows (idempotent) ──────────────────────────────────
        for (code, label, color, macro_state, is_terminal, sequence) in [
            ("planning", "Planning", "#3B82F6", "open", 0i64, 2i64),
            ("ready", "Ready", "#8B5CF6", "open", 0, 3),
            ("on_hold", "On Hold", "#F97316", "executing", 0, 5),
            ("completed", "Completed", "#06B6D4", "completed", 0, 6),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO work_order_statuses \
                 (code, label, color, macro_state, is_terminal, is_system, sequence) \
                 VALUES (?, ?, ?, ?, ?, 1, ?) \
                 ON CONFLICT(code) DO UPDATE SET \
                   label = excluded.label, \
                   color = excluded.color, \
                   macro_state = excluded.macro_state, \
                   is_terminal = excluded.is_terminal, \
                   is_system = 1, \
                   sequence = excluded.sequence",
                [
                    code.into(),
                    label.into(),
                    color.into(),
                    macro_state.into(),
                    is_terminal.into(),
                    sequence.into(),
                ],
            ))
            .await?;
        }

        // Refresh sequences / macros for kept codes
        for (code, label, color, macro_state, is_terminal, sequence) in [
            ("draft", "Draft", "#94A3B8", "open", 0i64, 1i64),
            ("in_progress", "In Progress", "#10B981", "executing", 0, 4),
            ("closed", "Closed", "#64748B", "closed", 1, 7),
            ("cancelled", "Cancelled", "#DC2626", "cancelled", 1, 8),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_statuses SET \
                   label = ?, color = ?, macro_state = ?, is_terminal = ?, \
                   is_system = 1, sequence = ? \
                 WHERE code = ?",
                [
                    label.into(),
                    color.into(),
                    macro_state.into(),
                    is_terminal.into(),
                    sequence.into(),
                    code.into(),
                ],
            ))
            .await?;
        }

        // ── Remap work_orders to new status codes ─────────────────────────
        // awaiting_approval / planned / ready_to_schedule / assigned → planning
        for old in ["awaiting_approval", "planned", "ready_to_schedule", "assigned"] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = 'planning' LIMIT 1\
                 ) WHERE status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1\
                 )",
                [old.into()],
            ))
            .await?;
        }

        // waiting_for_prerequisite / paused → on_hold
        for old in ["waiting_for_prerequisite", "paused"] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = 'on_hold' LIMIT 1\
                 ) WHERE status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1\
                 )",
                [old.into()],
            ))
            .await?;
        }

        // mechanically_complete / technically_verified → completed
        for old in ["mechanically_complete", "technically_verified"] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = 'completed' LIMIT 1\
                 ) WHERE status_id = (\
                   SELECT id FROM work_order_statuses WHERE code = ? LIMIT 1\
                 )",
                [old.into()],
            ))
            .await?;
        }

        // Soft-retire legacy status rows (keep for historical transition logs)
        for old in [
            "awaiting_approval",
            "planned",
            "ready_to_schedule",
            "assigned",
            "waiting_for_prerequisite",
            "paused",
            "mechanically_complete",
            "technically_verified",
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_order_statuses SET is_system = 0, sequence = sequence + 100 \
                 WHERE code = ?",
                [old.into()],
            ))
            .await?;
        }

        // ── planning_approved_at for approval_required readiness rule ─────
        let _ = db
            .execute_unprepared("ALTER TABLE work_orders ADD COLUMN planning_approved_at TEXT")
            .await;

        let _ = db
            .execute_unprepared("ALTER TABLE work_orders ADD COLUMN planning_approved_by_id INTEGER")
            .await;

        // ── Action / event log ────────────────────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS wo_action_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                wo_id INTEGER NOT NULL,
                action_code TEXT NOT NULL,
                actor_id INTEGER,
                acted_at TEXT NOT NULL,
                from_status TEXT,
                to_status TEXT,
                payload_json TEXT,
                FOREIGN KEY (wo_id) REFERENCES work_orders(id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wo_action_events_wo_acted \
             ON wo_action_events (wo_id, acted_at)",
        )
        .await?;

        // Tenant policy default: approval not required (rule returns N/A)
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO app_settings \
             (setting_key, category, setting_scope, setting_value_json, \
              setting_risk, validation_status, last_modified_at) \
             VALUES \
             ('wo_planning_approval_required', 'wo', 'tenant', 'false', \
              'low', 'valid', strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Irreversible data remap — leave as no-op.
        Ok(())
    }
}

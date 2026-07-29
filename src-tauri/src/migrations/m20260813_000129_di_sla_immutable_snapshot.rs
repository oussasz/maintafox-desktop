//! Migration 129 — Immutable DI SLA snapshot + breach notify flags.
//!
//! Adds frozen SLA targets/deadlines on `intervention_requests` so rule edits
//! never rewrite historical obligations. Extends `di_review_events` with
//! resolution snapshot columns. Backfills existing DIs from current urgency-only
//! active rules (best-effort one-shot freeze).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260813_000129_di_sla_immutable_snapshot"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── intervention_requests SLA freeze columns ──────────────────────
        for sql in [
            "ALTER TABLE intervention_requests ADD COLUMN sla_rule_id INTEGER",
            "ALTER TABLE intervention_requests ADD COLUMN sla_target_response_hours INTEGER",
            "ALTER TABLE intervention_requests ADD COLUMN sla_target_resolution_hours INTEGER",
            "ALTER TABLE intervention_requests ADD COLUMN sla_escalation_threshold_hours INTEGER",
            "ALTER TABLE intervention_requests ADD COLUMN sla_response_deadline TEXT",
            "ALTER TABLE intervention_requests ADD COLUMN sla_resolution_deadline TEXT",
            "ALTER TABLE intervention_requests ADD COLUMN sla_response_breach_notified_at TEXT",
            "ALTER TABLE intervention_requests ADD COLUMN sla_resolution_breach_notified_at TEXT",
            "ALTER TABLE di_review_events ADD COLUMN sla_resolution_target_hours INTEGER",
            "ALTER TABLE di_review_events ADD COLUMN sla_resolution_deadline TEXT",
        ] {
            db.execute_unprepared(sql).await?;
        }

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ir_sla_poll \
             ON intervention_requests (status, sla_response_deadline, sla_resolution_deadline)",
        )
        .await?;

        // Backfill: urgency-only broad match (origin/criticality NULL on rule).
        // Freezes then-current rules once; subsequent rule edits do not rewrite.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE intervention_requests AS ir
             SET
               sla_rule_id = (
                 SELECT r.id FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id
                 LIMIT 1
               ),
               sla_target_response_hours = (
                 SELECT r.target_response_hours FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id LIMIT 1
               ),
               sla_target_resolution_hours = (
                 SELECT r.target_resolution_hours FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id LIMIT 1
               ),
               sla_escalation_threshold_hours = (
                 SELECT r.escalation_threshold_hours FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id LIMIT 1
               ),
               sla_response_deadline = (
                 SELECT strftime(
                   '%Y-%m-%dT%H:%M:%SZ',
                   datetime(ir.submitted_at, '+' || r.target_response_hours || ' hours')
                 )
                 FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id LIMIT 1
               ),
               sla_resolution_deadline = (
                 SELECT strftime(
                   '%Y-%m-%dT%H:%M:%SZ',
                   datetime(ir.submitted_at, '+' || r.target_resolution_hours || ' hours')
                 )
                 FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
                 ORDER BY r.id LIMIT 1
               )
             WHERE ir.sla_response_deadline IS NULL
               AND EXISTS (
                 SELECT 1 FROM di_sla_rules r
                 WHERE r.urgency_level = ir.reported_urgency
                   AND r.is_active = 1
                   AND r.origin_type IS NULL
                   AND r.asset_criticality_class IS NULL
               )"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite cannot DROP COLUMN portably across all deployments; leave additive.
        let _ = manager;
        Ok(())
    }
}

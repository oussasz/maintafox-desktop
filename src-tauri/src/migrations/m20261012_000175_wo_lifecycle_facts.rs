//! Migration 175 — facts for the derived PRD 12-state WO lifecycle (Phase 6.4, option B;
//! `wo::lifecycle`). Additive only: the 8 stored phases and their codes are unchanged.
//!
//! - `work_orders.planning_approval_required`: whether this WO needs planning approval. Set when
//!   the WO enters planning, from the tenant setting `wo_planning_approval_required` at that
//!   moment. Back-filled here for every existing WO from the setting's current value.
//! - `work_order_delay_segments.hold_kind` (`pause` | `prerequisite`, never NULL): written by the
//!   hold action. Back-fill: `prerequisite` when the segment's delay reason is one of the
//!   prerequisite reasons of `WORK.DELAY_REASONS` — `no_parts`, `backordered`, `no_permit`,
//!   `permit_expired`, `no_shutdown`, `vendor_delay`, `no_access`, `no_labor` (missing parts,
//!   permit, shutdown window, vendor, access or people: PRD §6.5 "missing prerequisites") —
//!   otherwise `pause` (`diagnosis`, `other`, and any tenant-defined reason).
//! - `wo_state_transition_log.lifecycle_state`: the lifecycle state right after each transition
//!   (NULL for transitions logged before this version).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261012_000175_wo_lifecycle_facts"
    }
}

/// Delay reason codes that mean "waiting for a prerequisite".
pub const PREREQUISITE_REASON_CODES: &[&str] = &[
    "no_parts",
    "backordered",
    "no_permit",
    "permit_expired",
    "no_shutdown",
    "vendor_delay",
    "no_access",
    "no_labor",
];

async fn column_exists<C: ConnectionTrait>(db: &C, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// The tenant setting as `load_approval_policy` reads it (`true`, `"true"`, `1`, `yes`).
async fn approval_setting<C: ConnectionTrait>(db: &C) -> Result<bool, DbErr> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings \
             WHERE setting_key = 'wo_planning_approval_required' LIMIT 1"
                .to_string(),
        ))
        .await?;
    let raw: String = match row {
        Some(r) => r.try_get("", "setting_value_json").unwrap_or_default(),
        None => return Ok(false),
    };
    let v = raw.trim().trim_matches('"').to_ascii_lowercase();
    Ok(matches!(v.as_str(), "true" | "1" | "yes"))
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        if !column_exists(db, "work_orders", "planning_approval_required").await? {
            db.execute_unprepared(
                "ALTER TABLE work_orders ADD COLUMN planning_approval_required INTEGER NOT NULL DEFAULT 0",
            )
            .await?;
            let required = approval_setting(db).await?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_orders SET planning_approval_required = ?",
                [i64::from(required).into()],
            ))
            .await?;
        }

        if !column_exists(db, "work_order_delay_segments", "hold_kind").await? {
            db.execute_unprepared(
                "ALTER TABLE work_order_delay_segments ADD COLUMN hold_kind TEXT NOT NULL DEFAULT 'pause' \
                 CHECK (hold_kind IN ('pause', 'prerequisite'))",
            )
            .await?;
            let placeholders = vec!["?"; PREREQUISITE_REASON_CODES.len()].join(", ");
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "UPDATE work_order_delay_segments SET hold_kind = 'prerequisite' \
                     WHERE delay_reason_id IN (SELECT id FROM reference_values WHERE code IN ({placeholders}))"
                ),
                PREREQUISITE_REASON_CODES
                    .iter()
                    .map(|c| (*c).into())
                    .collect::<Vec<sea_orm::Value>>(),
            ))
            .await?;
        }

        if !column_exists(db, "wo_state_transition_log", "lifecycle_state").await? {
            db.execute_unprepared("ALTER TABLE wo_state_transition_log ADD COLUMN lifecycle_state TEXT NULL")
                .await?;
        }
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Additive columns only.
        Ok(())
    }
}

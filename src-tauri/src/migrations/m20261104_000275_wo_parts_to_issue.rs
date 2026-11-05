//! Migration 275 — work-order parts "à sortir" (L3-21, CMMS Advisor option A).
//!
//! Only the stock right (`inv.manage`) moves stock. A person without it who records the use of a
//! stock article on a work order records the line without a stock movement; the storekeeper then
//! issues it from the issue desk, or cancels it with a reason (`wo::parts`).
//!
//! - `work_order_parts.pending_issue_qty` (synced, `REAL NOT NULL DEFAULT 0`): the used quantity
//!   still to leave the store. Every existing line has 0: nothing recorded before this version is
//!   "à sortir" (its stock moved, or it had none, as before).
//! - The notification category `wo_part_to_issue` and its rule (audience `permission:inv.manage`):
//!   the stock right holders, at the work order's site, are told when a line is "à sortir". Seeded
//!   identities on every PC with capture suppressed, as migration 266: nothing is sent.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261104_000275_wo_parts_to_issue"
    }
}

const CATEGORY: &str = crate::wo::parts::PART_TO_ISSUE_CATEGORY;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let has_column = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS present FROM pragma_table_info('work_order_parts') WHERE name = 'pending_issue_qty'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if !has_column {
            txn.execute_unprepared("ALTER TABLE work_order_parts ADD COLUMN pending_issue_qty REAL NOT NULL DEFAULT 0")
                .await?;
        }

        // Made by this migration on every PC with the same identities: nothing is sent.
        let capture = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        let audiences = serde_json::json!([format!(
            "{}{}",
            crate::notifications::catalog::PERMISSION_AUDIENCE_PREFIX,
            crate::rbac::permissions::INV_MANAGE
        )])
        .to_string();
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, ?, 'info', 0, 1, 'inv', 1)",
            [
                seeded_entity_sync_id("notification_categories", CATEGORY).into(),
                CATEGORY.into(),
                "Work-order part to issue".into(),
            ],
        ))
        .await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO notification_rules \
             (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
              escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
             SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, ? \
             WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
            [
                seeded_entity_sync_id("notification_rules", CATEGORY).into(),
                CATEGORY.into(),
                audiences.into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for (table, column) in [
            ("notification_rules", "category_code"),
            ("notification_categories", "code"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                format!("DELETE FROM {table} WHERE {column} = ?"),
                [CATEGORY.into()],
            ))
            .await?;
        }
        Ok(())
    }
}

//! Migration 265 — work on retired equipment received by sync, to review (audit finding AU-73b,
//! CMMS Advisor decision; `wo::retired_equipment_review`).
//!
//! A work order or request created on one PC while its equipment was in service, and the
//! retirement (Déclassé / Rebuté) recorded on another, meet at sync: the received row is applied as
//! a field fact. Each such open work order or request is one review item "équipement déclassé".
//!
//! - `retired_equipment_reviews` keeps those items on this PC. It is a projection of synchronised
//!   state (the work order's or request's status, the equipment's lifecycle status) and is never
//!   synchronised: an item is open exactly while its record is open and its equipment retired, so
//!   every PC holding the same rows shows the same open items, keyed by the record's sync id
//!   (`record_kind`, `record_sync_id`). A closed item keeps how (`closure_kind`) and when
//!   (`closed_at`, the record's own time); who is read from the synchronised transition log or
//!   lifecycle event. The partial unique index allows one open item per record.
//!   `retired_at` keeps the retirement an item was opened under: its return to service is the first
//!   one after it. No foreign key to `equipment`: a projection never blocks a received delete.
//! - Two notification categories and their rules, one per module so each is seen only with that
//!   module's view right: `retired_equipment_work` (module `ot`: the work order's planner and the
//!   responsibles of its node) and `retired_equipment_request` (module `di`: the request's reviewer
//!   and the responsibles of its node), told once per item, in-app.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261026_000265_retired_equipment_reviews"
    }
}

const CATEGORY: &str = crate::wo::retired_equipment_review::NOTIFICATION_CATEGORY;
const REQUEST_CATEGORY: &str = crate::wo::retired_equipment_review::REQUEST_NOTIFICATION_CATEGORY;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for sql in [
            "CREATE TABLE IF NOT EXISTS retired_equipment_reviews (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                record_kind     TEXT    NOT NULL CHECK (record_kind IN ('work_order', 'intervention_request')),
                record_sync_id  TEXT    NOT NULL,
                record_id       INTEGER NOT NULL,
                equipment_id    INTEGER NOT NULL,
                status          TEXT    NOT NULL CHECK (status IN ('open', 'closed')),
                opened_at       TEXT    NOT NULL,
                retired_at      TEXT    NULL,
                closed_at       TEXT    NULL,
                closure_kind    TEXT    NULL CHECK (closure_kind IN
                    ('wo_closed', 'wo_cancelled', 'di_rejected', 'di_closed', 'di_converted', 'recommissioned', 'other')),
                CHECK ((status = 'open') = (closed_at IS NULL)),
                CHECK ((status = 'open') = (closure_kind IS NULL))
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_retired_equipment_reviews_open
                ON retired_equipment_reviews(record_kind, record_sync_id) WHERE status = 'open'",
            "CREATE INDEX IF NOT EXISTS idx_retired_equipment_reviews_equipment
                ON retired_equipment_reviews(equipment_id, status)",
        ] {
            txn.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
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
        for (category, label, module, audiences) in [
            (
                CATEGORY,
                "Work order on retired equipment",
                "ot",
                r#"["planner","entity_manager"]"#,
            ),
            (
                REQUEST_CATEGORY,
                "Request on retired equipment",
                "di",
                r#"["reviewer","entity_manager"]"#,
            ),
        ] {
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT OR IGNORE INTO notification_categories \
                 (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
                  module_code, is_offered) VALUES (?, ?, ?, 'warning', 0, 1, ?, 1)",
                [
                    seeded_entity_sync_id("notification_categories", category).into(),
                    category.into(),
                    label.into(),
                    module.into(),
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
                    seeded_entity_sync_id("notification_rules", category).into(),
                    category.into(),
                    audiences.into(),
                    category.into(),
                ],
            ))
            .await?;
        }
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "DROP TABLE IF EXISTS retired_equipment_reviews".to_string(),
        ))
        .await?;
        for (table, column) in [
            ("notification_rules", "category_code"),
            ("notification_categories", "code"),
        ] {
            for category in [CATEGORY, REQUEST_CATEGORY] {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    format!("DELETE FROM {table} WHERE {column} = ?"),
                    [category.into()],
                ))
                .await?;
            }
        }
        Ok(())
    }
}

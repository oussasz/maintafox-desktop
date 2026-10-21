//! Migration 251 — stock balances to reconcile after sync (CMMS Advisor 2026-10-20,
//! `inventory::negative_stock`).
//!
//! A stock movement received from another PC is applied as received (it is a physical fact): two
//! PCs that each issued the last units leave the location below zero on every PC. Such a balance
//! is "négatif — à rapprocher": one review item per article and location, until the stock there is
//! back to zero or more (a count, a manual entry counted as one, or a goods receipt).
//!
//! - `stock_negative_reviews` keeps those items on this PC. It is a projection of `stock_balances`
//!   (itself derived from the synchronised movement ledger) and is never synchronised: an item is
//!   open exactly while the location's on-hand stock is below zero, so every PC holding the same
//!   ledger shows the same open items. A closed item keeps how it was closed (`closure_kind`: a
//!   count, an adjustment with its reason, a goods receipt — "régularisé par réception" —, a
//!   documented return — "régularisé par retour" —, a manual entry made on a PC that did not yet
//!   see the location below zero — "régularisé par saisie antérieure" —, or none on record) and
//!   the movement that closed it. The partial unique index allows one open item per article and
//!   location.
//! - `inventory_count_lines` gains the mark of every count from this version on, whatever path
//!   records it, set when its counted quantity and variance are fixed: `count_replica_id` (the sync
//!   replica id of the PC that recorded it, the origin sync itself uses), `count_follows_json` (the
//!   posted counts of that article and location this PC knew then and that no other count it knew
//!   had followed) and `count_path` (`reconciliation`: the stock-to-reconcile list;
//!   `count_session`: a count session). They travel with the line: two counts made on two PCs
//!   while the location stood below zero there, neither knowing the other even indirectly, show as
//!   a double count "à vérifier" on every PC (CMMS Advisor ruling S1), until a later count that
//!   knows both.
//! - The notification category of the watcher `stock_to_reconcile` and its rule: the inv.manage
//!   holders are told once per item, in-app.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

use crate::sync::identity::seeded_entity_sync_id;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000251_stock_negative_reviews"
    }
}

const CATEGORY: &str = crate::inventory::negative_stock::NOTIFICATION_CATEGORY;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for sql in [
            "CREATE TABLE IF NOT EXISTS stock_negative_reviews (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                location_id             INTEGER NOT NULL REFERENCES stock_locations(id),
                status                  TEXT    NOT NULL CHECK (status IN ('open', 'closed')),
                opened_at               TEXT    NOT NULL,
                opening_transaction_id  INTEGER NULL REFERENCES inventory_transactions(id),
                closed_at               TEXT    NULL,
                closure_kind            TEXT    NULL
                    CHECK (closure_kind IN ('count', 'adjustment', 'receipt', 'return', 'prior_entry', 'other')),
                closing_transaction_id  INTEGER NULL REFERENCES inventory_transactions(id),
                CHECK ((status = 'open') = (closed_at IS NULL)),
                CHECK ((status = 'open') = (closure_kind IS NULL))
            )",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_stock_negative_reviews_open
                ON stock_negative_reviews(article_id, location_id) WHERE status = 'open'",
            "CREATE INDEX IF NOT EXISTS idx_stock_negative_reviews_pair
                ON stock_negative_reviews(article_id, location_id)",
        ] {
            txn.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
                .await?;
        }
        // Re-runnable: a column already there is kept.
        for column in ["count_replica_id", "count_follows_json", "count_path"] {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('inventory_count_lines') WHERE name = ?",
                    [column.into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!(
                    "ALTER TABLE inventory_count_lines ADD COLUMN {column} TEXT NULL"
                ))
                .await?;
            }
        }
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO notification_categories \
             (entity_sync_id, code, label, default_severity, default_requires_ack, is_user_configurable, \
              module_code, is_offered) VALUES (?, ?, 'Stock to reconcile after sync', 'warning', 0, 1, 'inv', 1)",
            [
                seeded_entity_sync_id("notification_categories", CATEGORY).into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO notification_rules \
             (entity_sync_id, category_code, routing_mode, requires_ack, dedupe_window_minutes, \
              escalation_policy_id, is_active, lead_time_minutes, audiences_json) \
             SELECT ?, ?, 'manual', 0, 0, NULL, 1, NULL, '[\"permission:inv.manage\"]' \
             WHERE NOT EXISTS (SELECT 1 FROM notification_rules WHERE category_code = ?)",
            [
                seeded_entity_sync_id("notification_rules", CATEGORY).into(),
                CATEGORY.into(),
                CATEGORY.into(),
            ],
        ))
        .await?;
        txn.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "DROP TABLE IF EXISTS stock_negative_reviews".to_string(),
        ))
        .await?;
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
        // The three count-line columns stay (SQLite cannot drop them on every supported version).
        Ok(())
    }
}

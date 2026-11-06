//! Migration 280 — hors-stock purchase charged to a work order (L3-04, CMMS Advisor option B).
//!
//! A hors-stock article is an article whose stocking type (system reference list
//! `inventory.stocking_type`) is `NON_STOCK`: no new flag, the list already holds the value.
//! It is bought for one work order, received without any stock movement and booked straight to
//! that work order as parts actual (`inventory::non_stock`).
//!
//! - `articles.origin_work_order_id` (synced, NULL): the work order the article was created from.
//! - `articles.catalogue_review_state` (synced, NULL | `TO_REVIEW` | `REVIEWED` | `DUPLICATE`):
//!   the catalogue manager's review of an article created from a work order. Every existing
//!   article stays NULL (not created from a work order).
//! - `articles.duplicate_of_article_id` (synced, NULL): the article it duplicates, when marked so.
//!   Nothing is ever deleted.
//! - `work_order_parts.non_stock_received_qty` (synced, `REAL NOT NULL DEFAULT 0`): quantity of a
//!   hors-stock line received from the supplier ("reçu"). Existing lines have 0.
//! - `work_order_parts.handed_to_personnel_id` (synced, NULL): optional "remis à <personne>".

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261105_000280_non_stock_wo_purchase"
    }
}

const COLUMNS: &[(&str, &str, &str)] = &[
    ("articles", "origin_work_order_id", "INTEGER NULL"),
    ("articles", "catalogue_review_state", "TEXT NULL"),
    ("articles", "duplicate_of_article_id", "INTEGER NULL"),
    ("work_order_parts", "non_stock_received_qty", "REAL NOT NULL DEFAULT 0"),
    ("work_order_parts", "handed_to_personnel_id", "INTEGER NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for (table, column, definition) in COLUMNS {
            let present = txn
                .query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    format!("SELECT 1 AS present FROM pragma_table_info('{table}') WHERE name = '{column}'"),
                ))
                .await?
                .is_some();
            if !present {
                txn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))
                    .await?;
            }
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_articles_catalogue_review_state ON articles(catalogue_review_state)",
        )
        .await?;
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_articles_catalogue_review_state")
            .await?;
        Ok(())
    }
}

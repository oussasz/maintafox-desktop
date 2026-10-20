//! Migration 245 — "Assign a site" to old records without one (CMMS Advisor decision Q1, step 6).
//!
//! Work orders, requests and planning windows recorded before a site was required may have none
//! (`work_orders.entity_id` NULL, `intervention_requests.org_node_id` 0, `planning_windows.entity_id`
//! NULL). A plant-wide organisation administrator may give such a record a site, once, with a reason
//! (`org::site_assignment`). The record keeps the trace of that assignment, which travels with the
//! row so the other PCs re-check it on arrival (the author holds the right there and the record still
//! has no site there, else a sync conflict):
//! - `site_assigned_by_id`: who gave the site (a user reference, sent as a user sync id);
//! - `site_assigned_at`: when (UTC);
//! - `site_assign_reason`: why.
//!
//! All nullable: every existing record keeps NULL (no site was assigned this way). Nothing is
//! rewritten: the site stays empty until an administrator assigns one.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000245_legacy_site_assignment"
    }
}

const TABLES: &[&str] = &["work_orders", "intervention_requests", "planning_windows"];

const COLUMNS: &[(&str, &str)] = &[
    ("site_assigned_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("site_assigned_at", "TEXT NULL"),
    ("site_assign_reason", "TEXT NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for table in TABLES {
            for (column, ddl) in COLUMNS {
                let exists = txn
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        format!("SELECT 1 AS ok FROM pragma_table_info('{table}') WHERE name = ?"),
                        [(*column).into()],
                    ))
                    .await?
                    .is_some();
                if !exists {
                    txn.execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"))
                        .await?;
                }
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The trace of an assigned site: kept.
        Ok(())
    }
}

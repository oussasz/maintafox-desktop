//! Migration 046 - Inventory cycle count governance and reconciliation controls.

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260421_000046_inventory_cycle_count_and_reconciliation_controls"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_count_sessions (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                session_code            TEXT    NOT NULL UNIQUE,
                warehouse_id            INTEGER NOT NULL REFERENCES warehouses(id),
                location_id             INTEGER NULL REFERENCES stock_locations(id),
                status                  TEXT    NOT NULL DEFAULT 'draft',
                critical_abs_threshold  REAL    NOT NULL DEFAULT 5,
                opened_by_id            INTEGER NULL,
                submitted_by_id         INTEGER NULL,
                submitted_at            TEXT    NULL,
                posted_by_id            INTEGER NULL,
                posted_at               TEXT    NULL,
                reversed_by_id          INTEGER NULL,
                reversed_at             TEXT    NULL,
                reversal_reason         TEXT    NULL,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_count_lines (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id              INTEGER NOT NULL REFERENCES inventory_count_sessions(id),
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                warehouse_id            INTEGER NOT NULL REFERENCES warehouses(id),
                location_id             INTEGER NOT NULL REFERENCES stock_locations(id),
                system_qty              REAL    NOT NULL,
                counted_qty             REAL    NOT NULL,
                variance_qty            REAL    NOT NULL,
                variance_reason_code    TEXT    NULL,
                is_critical             INTEGER NOT NULL DEFAULT 0,
                approval_required       INTEGER NOT NULL DEFAULT 0,
                approved_by_id          INTEGER NULL,
                approved_at             TEXT    NULL,
                approval_note           TEXT    NULL,
                posted_transaction_id   INTEGER NULL REFERENCES inventory_transactions(id),
                reversed_transaction_id INTEGER NULL REFERENCES inventory_transactions(id),
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(session_id, article_id, location_id)
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_mutation_audit_links (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                transaction_id          INTEGER NOT NULL REFERENCES inventory_transactions(id),
                source_entity_type      TEXT    NOT NULL,
                source_entity_id        INTEGER NOT NULL,
                reviewer_id             INTEGER NULL,
                reviewer_evidence       TEXT    NULL,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_reconciliation_runs (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                run_code                TEXT    NOT NULL UNIQUE,
                run_date                TEXT    NOT NULL,
                status                  TEXT    NOT NULL DEFAULT 'completed',
                checked_rows            INTEGER NOT NULL DEFAULT 0,
                drift_rows              INTEGER NOT NULL DEFAULT 0,
                checked_by_id           INTEGER NULL,
                started_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                finished_at             TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_reconciliation_findings (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id                   INTEGER NOT NULL REFERENCES inventory_reconciliation_runs(id),
                article_id               INTEGER NOT NULL REFERENCES articles(id),
                warehouse_id             INTEGER NOT NULL REFERENCES warehouses(id),
                location_id              INTEGER NOT NULL REFERENCES stock_locations(id),
                balance_on_hand          REAL    NOT NULL,
                ledger_expected_on_hand  REAL    NOT NULL,
                drift_qty                REAL    NOT NULL,
                is_break                 INTEGER NOT NULL DEFAULT 0,
                created_at               TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_count_lines_session ON inventory_count_lines(session_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_count_lines_posted ON inventory_count_lines(posted_transaction_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_mutation_links_tx ON inventory_mutation_audit_links(transaction_id)".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_recon_findings_run ON inventory_reconciliation_findings(run_id)".to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_reconciliation_findings".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_reconciliation_runs".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_mutation_audit_links".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_count_lines".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_count_sessions".to_string(),
        ))
        .await?;
        Ok(())
    }
}

//! Migration 025 — WO close-out, verification, and attachment tables.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S1.
//!
//! Creates:
//!   - `work_order_failure_details`: structured failure taxonomy linked to reference_values
//!   - `work_order_verifications`: per-WO verification records with result + recurrence risk
//!   - `work_order_attachments`: file metadata for photos, reports, and PDF work sheets
//!
//! Alters `work_orders`:
//!   - `service_cost_input REAL NULL DEFAULT 0` — manual vendor/service cost entry
//!   - `reopen_count INTEGER NOT NULL DEFAULT 0` — incremented on each reopen
//!   - `last_closed_at TEXT NULL` — preserved timestamp from prior closure on reopen

use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260410_000025_wo_closeout_and_attachments"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── work_order_failure_details ─────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_failure_details (
                id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id        INTEGER NOT NULL REFERENCES work_orders(id),
                symptom_id           INTEGER NULL REFERENCES reference_values(id),
                failure_mode_id      INTEGER NULL REFERENCES reference_values(id),
                failure_cause_id     INTEGER NULL REFERENCES reference_values(id),
                failure_effect_id    INTEGER NULL REFERENCES reference_values(id),
                is_temporary_repair  INTEGER NOT NULL DEFAULT 0,
                is_permanent_repair  INTEGER NOT NULL DEFAULT 0,
                cause_not_determined INTEGER NOT NULL DEFAULT 0,
                notes                TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_verifications ──────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_verifications (
                id                          INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id               INTEGER NOT NULL REFERENCES work_orders(id),
                verified_by_id              INTEGER NOT NULL REFERENCES user_accounts(id),
                verified_at                 TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                result                      TEXT    NOT NULL,
                return_to_service_confirmed INTEGER NOT NULL DEFAULT 0,
                recurrence_risk_level       TEXT    NULL,
                notes                       TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── work_order_attachments ────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS work_order_attachments (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id  INTEGER NOT NULL REFERENCES work_orders(id),
                file_name      TEXT    NOT NULL,
                relative_path  TEXT    NOT NULL UNIQUE,
                mime_type      TEXT    NOT NULL DEFAULT 'application/octet-stream',
                size_bytes     INTEGER NOT NULL DEFAULT 0,
                uploaded_by_id INTEGER NULL REFERENCES user_accounts(id),
                uploaded_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                notes          TEXT    NULL
            )"
            .to_string(),
        ))
        .await?;

        // ── Indexes ───────────────────────────────────────────────────────
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wofd_wo_id ON work_order_failure_details(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_wov_wo_id ON work_order_verifications(work_order_id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_woa_wo_id ON work_order_attachments(work_order_id)"
                .to_string(),
        ))
        .await?;

        // ── Add columns to work_orders ────────────────────────────────────
        // NOTE: parts_actuals_confirmed already added by migration 023.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_orders ADD COLUMN service_cost_input REAL NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_orders ADD COLUMN reopen_count INTEGER NOT NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE work_orders ADD COLUMN last_closed_at TEXT NULL"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for table in &[
            "work_order_attachments",
            "work_order_verifications",
            "work_order_failure_details",
        ] {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                format!("DROP TABLE IF EXISTS {table}"),
            ))
            .await?;
        }

        // SQLite 3.35+ DROP COLUMN
        for col in &["service_cost_input", "reopen_count", "last_closed_at"] {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                format!("ALTER TABLE work_orders DROP COLUMN IF EXISTS {col}"),
            ))
            .await?;
        }

        Ok(())
    }
}

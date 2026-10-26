//! Migration 264 — the approval seal of a reliability study decision, and its refusal by sync
//! (audit finding AU-76b, CMMS Advisor answers 2026-10-26).
//!
//! - `approval_seal`: written when a decision is approved, `v1:` + SHA-256 of the decision's
//!   content in its synchronised form (`reliability::study::decision_sync::approval_seal`): what
//!   was proposed, who proposed it, who was designated to approve it, who approved it, when and
//!   with which note. A PC receiving the decision recomputes it: a decision changed after its
//!   approval is refused there. NULL: not approved (proposed, rejected), or approved by an app
//!   version that did not seal.
//! - `sync_refused_at` / `sync_refused_reason`: a decision refused by a person on a PC that
//!   received it ("keep this PC's decision" on its sync conflict): kept, sent back with these
//!   marks, never applied and its PM version never published on any PC.
//!
//! Backfill: every decision approved or applied on this PC gets its seal computed from its own
//! content (the same function as at approval), so every PC holding the same decision computes the
//! same seal. The decision is sent once more, sealed, only by the PC whose version the server holds
//! (or that has not sent it yet), so a PC that received it unsealed and waits for it gets it
//! ([`resend_sealed`]). A row that cannot be read is left unsealed and logged. Proposed and rejected
//! decisions keep NULL. No refusal is guessed. Already-present columns and seals are left alone (a
//! re-run changes nothing).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261026_000264_reliability_decision_seal"
    }
}

async fn has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

const COLUMNS: &[(&str, &str)] = &[
    ("approval_seal", "TEXT NULL"),
    ("sync_refused_at", "TEXT NULL"),
    ("sync_refused_reason", "TEXT NULL"),
];

async fn table_exists(conn: &impl ConnectionTrait, table: &str) -> Result<bool, DbErr> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table.into()],
        ))
        .await?
        .is_some())
}

/// Sends the sealed decision once more when this PC is the one whose version the server holds
/// (the server acknowledged it from here, `sync_entity_state.source = 'outbox_ack'`), or when its
/// unsealed version is still waiting to be sent here (that version is dropped): a PC that received
/// the decision unsealed from this PC waits for it (`SENDER_OUTDATED`, review S7). One PC per
/// decision re-sends it, on top of the version the server holds, so no other PC is in conflict;
/// the PCs holding it already compute the same seal and keep it as it is. The capture drain after
/// start-up stages it (`sync::capture::drain_capture_journal`), as migration 201 does.
async fn resend_sealed(conn: &impl ConnectionTrait, sync_id: &str) -> Result<(), DbErr> {
    let pending = conn
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM sync_outbox WHERE entity_type = 'reliability_study_decisions' AND entity_sync_id = ? \
               AND status = 'pending' AND operation NOT IN ('delete', 'archive')",
            [sync_id.into()],
        ))
        .await?
        .rows_affected();
    let last_writer = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS w FROM sync_entity_state \
              WHERE entity_type = 'reliability_study_decisions' AND entity_sync_id = ? AND source = 'outbox_ack'",
            [sync_id.into()],
        ))
        .await?
        .is_some();
    if pending > 0 || last_writer {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, \
               owner_user_sync_id) VALUES ('reliability_study_decisions', ?, NULL, 'upsert', NULL)",
            [sync_id.into()],
        ))
        .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here, every part guarded.
        let txn = manager.get_connection().begin().await?;
        for (column, definition) in COLUMNS {
            if !has_column(&txn, "reliability_study_decisions", column).await? {
                txn.execute_unprepared(&format!(
                    "ALTER TABLE reliability_study_decisions ADD COLUMN {column} {definition}"
                ))
                .await?;
            }
        }
        let rows = txn
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id, entity_sync_id FROM reliability_study_decisions \
                  WHERE status IN ('approved', 'applied') AND approval_seal IS NULL",
            ))
            .await?;
        let resend = table_exists(&txn, "sync_capture_journal").await?
            && table_exists(&txn, "sync_entity_state").await?
            && table_exists(&txn, "sync_outbox").await?;
        for row in rows {
            let id: i64 = row.try_get("", "id")?;
            let sync_id: Option<String> = row.try_get("", "entity_sync_id")?;
            // A row that cannot be read is left unsealed (judged by comparison where it is held), never
            // a reason to stop the upgrade (review N2).
            let seal = match crate::reliability::study::decision_sync::local_approval_seal(&txn, id).await {
                Ok(Some(seal)) => seal,
                Ok(None) => continue,
                Err(err) => {
                    tracing::warn!(decision_id = id, error = %err, "migration 264: decision left unsealed");
                    continue;
                }
            };
            // The row's version and time are left as they are: the seal is the same on every PC
            // holding the same decision.
            txn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE reliability_study_decisions SET approval_seal = ? WHERE id = ? AND approval_seal IS NULL",
                [seal.into(), id.into()],
            ))
            .await?;
            let Some(sync_id) = sync_id.filter(|s| !s.trim().is_empty()) else {
                continue;
            };
            if resend {
                resend_sealed(&txn, &sync_id).await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

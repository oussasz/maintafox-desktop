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
//!   marks; from then on no PC approves or applies it or publishes a PM version from it (what it
//!   produced before stays, `docs/KNOWN_LIMITS.md`).
//!
//! Backfill: every decision approved or applied on this PC gets its seal computed from its own
//! content (the same function as at approval), so every PC holding the same decision computes the
//! same seal. A sealed decision with a change not sent yet here is journaled on top of that change;
//! any other is listed to be sent once more, sealed, after the first complete pull that follows the
//! upgrade, by the PC whose version the server then holds (or that never sent it), so a PC that
//! received it unsealed and waits for it gets it ([`arrange_resend`]). Nothing unsent is dropped. A row that
//! cannot be read is left unsealed and logged. Proposed and rejected
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

/// Arranges for the sealed decision to reach the other PCs sealed (review S7, R2-S8, R3-B1):
/// - a change of it not sent yet here (pending, or sent and not acknowledged) is a real local change:
///   it is kept as it is, and the sealed current row is journaled on top of it, so the capture drain
///   after start-up stages it as the next version after that change (`sync::capture`, base = the
///   unsent version), whoever the server's last writer is: nothing local is ever dropped;
/// - otherwise it is listed to be sent once more after the first complete pull that follows this
///   upgrade (`reliability::study::decision_sync::resend_sealed_after_complete_pull`): only then does
///   this PC know whether the version the server holds is still its own; before any pull that
///   knowledge may be stale, and a re-send of an old version would be a conflict on a PC that
///   changed nothing.
async fn arrange_resend(
    conn: &impl ConnectionTrait,
    sync_id: &str,
    journal: bool,
    marked: &mut Vec<String>,
) -> Result<(), DbErr> {
    let unsent = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS u FROM sync_outbox WHERE entity_type = 'reliability_study_decisions' AND entity_sync_id = ? \
               AND status IN ('pending', 'in_flight') AND operation NOT IN ('delete', 'archive') LIMIT 1",
            [sync_id.into()],
        ))
        .await?
        .is_some();
    if unsent && journal {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, natural_key_json, operation, \
               owner_user_sync_id) VALUES ('reliability_study_decisions', ?, NULL, 'upsert', NULL)",
            [sync_id.into()],
        ))
        .await?;
    } else if !unsent && !marked.iter().any(|s| s == sync_id) {
        marked.push(sync_id.to_string());
    }
    Ok(())
}

/// Stores the decisions to re-send after the next complete pull (`system_config`, local to this
/// PC), keeping those already listed by an earlier run.
async fn store_resend_list(conn: &impl ConnectionTrait, mut marked: Vec<String>) -> Result<(), DbErr> {
    if marked.is_empty() {
        return Ok(());
    }
    let key = crate::reliability::study::decision_sync::SEAL_RESEND_KEY;
    let held: Option<String> = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = ?",
            [key.into()],
        ))
        .await?
        .map(|r| r.try_get::<String>("", "value"))
        .transpose()?;
    if let Some(held) = held {
        for id in serde_json::from_str::<Vec<String>>(&held).unwrap_or_default() {
            if !marked.contains(&id) {
                marked.push(id);
            }
        }
    }
    let list = serde_json::to_string(&marked).map_err(|e| DbErr::Custom(e.to_string()))?;
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO system_config (key, value, updated_at) VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [key.into(), list.into()],
    ))
    .await?;
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
        let resend = table_exists(&txn, "sync_outbox").await? && table_exists(&txn, "system_config").await?;
        let journal = table_exists(&txn, "sync_capture_journal").await?;
        let mut marked = Vec::new();
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
                arrange_resend(&txn, &sync_id, journal, &mut marked).await?;
            }
        }
        if resend {
            store_resend_list(&txn, marked).await?;
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

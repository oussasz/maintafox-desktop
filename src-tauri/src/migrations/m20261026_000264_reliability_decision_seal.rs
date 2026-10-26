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
//! same seal; nothing is sent. Proposed and rejected decisions keep NULL. No refusal is guessed.
//! Already-present columns and seals are left alone (a re-run changes nothing).

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
                "SELECT id FROM reliability_study_decisions \
                  WHERE status IN ('approved', 'applied') AND approval_seal IS NULL",
            ))
            .await?;
        for row in rows {
            let id: i64 = row.try_get("", "id")?;
            let seal = crate::reliability::study::decision_sync::local_approval_seal(&txn, id)
                .await
                .map_err(|e| DbErr::Custom(format!("approval seal of decision {id}: {e}")))?;
            if let Some(seal) = seal {
                // The row's version and time are left as they are: the seal is the same on every PC
                // holding the decision, nothing is sent.
                txn.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE reliability_study_decisions SET approval_seal = ? WHERE id = ? AND approval_seal IS NULL",
                    [seal.into(), id.into()],
                ))
                .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

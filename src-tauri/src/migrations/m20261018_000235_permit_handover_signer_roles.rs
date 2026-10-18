//! Migration 235 — a permit hand-over records who signed it and its roles come from a list.
//!
//! - `permit_handover_logs.signed_by_id`: the person who recorded the hand-over, set by the server
//!   from the session (never from the screen). `received_by_id`: the receiving person, optional,
//!   picked from the user accounts. Both nullable: hand-overs recorded before keep NULL — nobody is
//!   invented after the fact. Both sync with the row as user references.
//! - `PERMIT.HANDOVER_ROLES` (COMPANY list, Données de référence) is created **empty**. The free
//!   texts already stored as "from" / "to" roles are adopted with their exact text and flagged
//!   "à revoir"; no stored hand-over is rewritten (`permit::handover_roles`, same pattern as 229).
//!   The integrity pass runs the same adoption for texts received later by sync.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000235_permit_handover_signer_roles"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("signed_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("received_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('permit_handover_logs') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("ALTER TABLE permit_handover_logs ADD COLUMN {column} {ddl}"),
                ))
                .await?;
            }
        }
        crate::permit::handover_roles::adopt_stored_roles(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Signers and adopted roles describe stored data: kept.
        Ok(())
    }
}

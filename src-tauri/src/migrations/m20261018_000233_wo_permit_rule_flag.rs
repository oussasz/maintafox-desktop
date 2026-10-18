//! Migration 233 — a work order converted from a request is flagged "permit required" by the
//! plant's rules (owner decision: requests carry no permit field; the flag is set at conversion).
//!
//! - `PERMIT.REQUIREMENT_RULES` (COMPANY list, Données de référence) is created **empty**: which
//!   equipment classes and / or work types need a permit, and which permit type. With no rule
//!   nothing is flagged by a rule (the conversion response says so).
//! - `work_orders` gains the trace of the rule that flagged it, and of who cleared that flag:
//!   - `permit_rule_code` / `permit_rule_permit_type`: the rule that matched and its permit type;
//!   - `permit_rule_cleared_by_id` / `permit_rule_cleared_at` / `permit_rule_clear_reason`: set
//!     only by the clear command (`ptw.issue`, reason of 10 characters or more, audited).
//!
//!   All NULL on existing work orders: none was flagged by a rule (today's behaviour). The columns
//!   sync with the row (the layout reads the table's columns; `permit_rule_cleared_by_id` travels
//!   as a user reference).

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261018_000233_wo_permit_rule_flag"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("permit_rule_code", "TEXT NULL"),
    ("permit_rule_permit_type", "TEXT NULL"),
    ("permit_rule_cleared_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("permit_rule_cleared_at", "TEXT NULL"),
    ("permit_rule_clear_reason", "TEXT NULL"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('work_orders') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("ALTER TABLE work_orders ADD COLUMN {column} {ddl}"),
                ))
                .await?;
            }
        }
        crate::permit::requirement_rules::ensure_permit_requirement_rules(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Flags and their trace may already be set: kept.
        Ok(())
    }
}

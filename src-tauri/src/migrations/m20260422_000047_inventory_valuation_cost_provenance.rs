//! Migration 047 - Inventory valuation and cost provenance

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260422_000047_inventory_valuation_cost_provenance"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_valuation_policies (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                code                    TEXT    NOT NULL UNIQUE,
                name                    TEXT    NOT NULL,
                scope_level             INTEGER NOT NULL,
                warehouse_id            INTEGER NULL REFERENCES warehouses(id),
                family_id               INTEGER NULL REFERENCES article_families(id),
                article_id              INTEGER NULL REFERENCES articles(id),
                valuation_method        TEXT    NOT NULL,
                currency_value_id       INTEGER NOT NULL REFERENCES lookup_values(id),
                standard_unit_cost      REAL    NULL,
                contract_ref            TEXT    NULL,
                sort_order              INTEGER NOT NULL DEFAULT 100,
                is_active               INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_val_policies_scope
             ON inventory_valuation_policies(scope_level, sort_order, is_active)"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS article_cost_profiles (
                article_id              INTEGER PRIMARY KEY REFERENCES articles(id),
                standard_unit_cost      REAL    NULL,
                currency_value_id       INTEGER NULL REFERENCES lookup_values(id),
                preferred_supplier_id   INTEGER NULL REFERENCES external_companies(id),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        for sql in [
            "ALTER TABLE stock_balances ADD COLUMN moving_avg_unit_cost REAL NULL",
            "ALTER TABLE stock_balances ADD COLUMN valuation_currency_value_id INTEGER NULL REFERENCES lookup_values(id)",
            "ALTER TABLE inventory_transactions ADD COLUMN unit_cost REAL NULL",
            "ALTER TABLE inventory_transactions ADD COLUMN extended_cost REAL NULL",
            "ALTER TABLE inventory_transactions ADD COLUMN cost_source_type TEXT NULL",
            "ALTER TABLE inventory_transactions ADD COLUMN cost_source_ref TEXT NULL",
            "ALTER TABLE inventory_transactions ADD COLUMN cost_currency_value_id INTEGER NULL REFERENCES lookup_values(id)",
            "ALTER TABLE inventory_transactions ADD COLUMN cost_effective_at TEXT NULL",
            "ALTER TABLE inventory_transactions ADD COLUMN is_provisional INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE inventory_transactions ADD COLUMN erp_reconcile_state TEXT NULL DEFAULT 'POSTED'",
            "ALTER TABLE inventory_transactions ADD COLUMN erp_reconcile_note TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN planned_unit_cost REAL NULL",
            "ALTER TABLE work_order_parts ADD COLUMN posted_unit_cost REAL NULL",
            "ALTER TABLE work_order_parts ADD COLUMN cost_source_type TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN cost_source_ref TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN cost_currency_value_id INTEGER NULL REFERENCES lookup_values(id)",
            "ALTER TABLE work_order_parts ADD COLUMN cost_effective_at TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN is_cost_override INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE work_order_parts ADD COLUMN cost_override_reason TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN cost_override_by_id INTEGER NULL",
            "ALTER TABLE work_order_parts ADD COLUMN cost_override_at TEXT NULL",
            "ALTER TABLE work_order_parts ADD COLUMN erp_reconcile_state TEXT NULL DEFAULT 'POSTED'",
            "ALTER TABLE work_order_parts ADD COLUMN erp_reconcile_note TEXT NULL",
            "ALTER TABLE procurement_requisition_lines ADD COLUMN projected_unit_cost REAL NULL",
            "ALTER TABLE procurement_requisition_lines ADD COLUMN projected_extended_cost REAL NULL",
            "ALTER TABLE procurement_requisition_lines ADD COLUMN projected_cost_currency_value_id INTEGER NULL REFERENCES lookup_values(id)",
            "ALTER TABLE procurement_requisition_lines ADD COLUMN projected_cost_confidence TEXT NULL",
            "ALTER TABLE procurement_requisition_lines ADD COLUMN projected_cost_basis TEXT NULL",
        ] {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await?;
        }

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE work_order_parts
             SET planned_unit_cost = unit_cost
             WHERE planned_unit_cost IS NULL AND unit_cost IS NOT NULL"
            .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS article_cost_profiles".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_valuation_policies".to_string(),
        ))
        .await?;
        Ok(())
    }
}
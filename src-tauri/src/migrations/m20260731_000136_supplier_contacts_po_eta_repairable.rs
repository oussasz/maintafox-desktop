//! Migration 136 — Supplier contacts, PO expected delivery, repairable maturity fields.

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260731_000136_supplier_contacts_po_eta_repairable"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_supplier_contacts (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                supplier_id     INTEGER NOT NULL REFERENCES inventory_suppliers(id),
                contact_name    TEXT    NOT NULL,
                contact_role    TEXT    NULL,
                phone           TEXT    NULL,
                email           TEXT    NULL,
                is_primary      INTEGER NOT NULL DEFAULT 0,
                created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inv_supplier_contacts_supplier
             ON inventory_supplier_contacts(supplier_id)"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE purchase_orders ADD COLUMN expected_delivery_date TEXT NULL".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN serial_number TEXT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN vendor_supplier_id INTEGER NULL REFERENCES inventory_suppliers(id)"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN sent_at TEXT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN returned_at TEXT NULL".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN warranty_active INTEGER NOT NULL DEFAULT 0"
                .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE repairable_orders ADD COLUMN warranty_until TEXT NULL".to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let _ = manager;
        Ok(())
    }
}

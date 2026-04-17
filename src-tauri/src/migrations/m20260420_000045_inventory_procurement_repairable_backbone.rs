//! Migration 045 - Inventory procurement and repairable backbone.
//!
//! Adds requisition / PO / GR lifecycle tables and repairable execution tables
//! with demand-source traceability and ERP posting-state compatibility.

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260420_000045_inventory_procurement_repairable_backbone"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS procurement_requisitions (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                req_number          TEXT    NOT NULL UNIQUE,
                demand_source_type  TEXT    NOT NULL,
                demand_source_id    INTEGER NULL,
                demand_source_ref   TEXT    NULL,
                status              TEXT    NOT NULL DEFAULT 'draft',
                posting_state       TEXT    NOT NULL DEFAULT 'pending_posting',
                posting_error       TEXT    NULL,
                requested_by_id     INTEGER NULL,
                row_version         INTEGER NOT NULL DEFAULT 1,
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS procurement_requisition_lines (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                requisition_id          INTEGER NOT NULL REFERENCES procurement_requisitions(id),
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                preferred_location_id   INTEGER NULL REFERENCES stock_locations(id),
                requested_qty           REAL    NOT NULL,
                source_reservation_id   INTEGER NULL REFERENCES stock_reservations(id),
                source_reorder_trigger  TEXT    NULL,
                status                  TEXT    NOT NULL DEFAULT 'open',
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS purchase_orders (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                po_number           TEXT    NOT NULL UNIQUE,
                requisition_id      INTEGER NULL REFERENCES procurement_requisitions(id),
                supplier_company_id INTEGER NULL REFERENCES external_companies(id),
                status              TEXT    NOT NULL DEFAULT 'draft',
                posting_state       TEXT    NOT NULL DEFAULT 'pending_posting',
                posting_error       TEXT    NULL,
                ordered_by_id       INTEGER NULL,
                ordered_at          TEXT    NULL,
                approved_by_id      INTEGER NULL,
                approved_at         TEXT    NULL,
                row_version         INTEGER NOT NULL DEFAULT 1,
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS purchase_order_lines (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                purchase_order_id       INTEGER NOT NULL REFERENCES purchase_orders(id),
                requisition_line_id     INTEGER NULL REFERENCES procurement_requisition_lines(id),
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                ordered_qty             REAL    NOT NULL,
                received_qty            REAL    NOT NULL DEFAULT 0,
                unit_price              REAL    NULL,
                demand_source_type      TEXT    NOT NULL,
                demand_source_id        INTEGER NULL,
                demand_source_ref       TEXT    NULL,
                source_reservation_id   INTEGER NULL REFERENCES stock_reservations(id),
                status                  TEXT    NOT NULL DEFAULT 'open',
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS goods_receipts (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                gr_number           TEXT    NOT NULL UNIQUE,
                purchase_order_id   INTEGER NOT NULL REFERENCES purchase_orders(id),
                status              TEXT    NOT NULL DEFAULT 'draft',
                posting_state       TEXT    NOT NULL DEFAULT 'pending_posting',
                posting_error       TEXT    NULL,
                received_by_id      INTEGER NULL,
                received_at         TEXT    NULL,
                row_version         INTEGER NOT NULL DEFAULT 1,
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS goods_receipt_lines (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                goods_receipt_id    INTEGER NOT NULL REFERENCES goods_receipts(id),
                po_line_id          INTEGER NOT NULL REFERENCES purchase_order_lines(id),
                article_id          INTEGER NOT NULL REFERENCES articles(id),
                location_id         INTEGER NOT NULL REFERENCES stock_locations(id),
                received_qty        REAL    NOT NULL,
                accepted_qty        REAL    NOT NULL,
                rejected_qty        REAL    NOT NULL DEFAULT 0,
                rejection_reason    TEXT    NULL,
                status              TEXT    NOT NULL DEFAULT 'received',
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS repairable_orders (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                order_code              TEXT    NOT NULL UNIQUE,
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                quantity                REAL    NOT NULL,
                source_location_id      INTEGER NOT NULL REFERENCES stock_locations(id),
                return_location_id      INTEGER NULL REFERENCES stock_locations(id),
                linked_po_line_id       INTEGER NULL REFERENCES purchase_order_lines(id),
                linked_reservation_id   INTEGER NULL REFERENCES stock_reservations(id),
                status                  TEXT    NOT NULL DEFAULT 'requested',
                reason                  TEXT    NULL,
                created_by_id           INTEGER NULL,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_state_events (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type         TEXT    NOT NULL,
                entity_id           INTEGER NOT NULL,
                from_status         TEXT    NULL,
                to_status           TEXT    NOT NULL,
                actor_id            INTEGER NULL,
                reason              TEXT    NULL,
                note                TEXT    NULL,
                changed_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_requisition_lines_requisition_id
             ON procurement_requisition_lines(requisition_id)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_po_lines_po_id
             ON purchase_order_lines(purchase_order_id)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_po_lines_trace
             ON purchase_order_lines(demand_source_type, demand_source_id)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_gr_lines_gr_id
             ON goods_receipt_lines(goods_receipt_id)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_repairable_orders_status
             ON repairable_orders(status)"
            .to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inventory_state_events_entity
             ON inventory_state_events(entity_type, entity_id, changed_at)"
            .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS inventory_state_events".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS repairable_orders".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS goods_receipt_lines".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS goods_receipts".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS purchase_order_lines".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS purchase_orders".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS procurement_requisition_lines".to_string(),
        ))
        .await?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE IF EXISTS procurement_requisitions".to_string(),
        ))
        .await?;
        Ok(())
    }
}

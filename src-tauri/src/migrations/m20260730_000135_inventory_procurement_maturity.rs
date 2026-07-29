//! Migration 135 - Procurement maturity backbone.
//!
//! Supplier master, multi-supplier article sourcing, item identity (manufacturer/MPN/OEM),
//! equivalent parts, replenishment policy fields, price lists, document links,
//! purchase priority, demand line FK, and GR lead-time / delivery accuracy columns.

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260730_000135_inventory_procurement_maturity"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // --- Supplier Master ---
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_suppliers (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                code                    TEXT    NOT NULL UNIQUE,
                name                    TEXT    NOT NULL,
                external_company_id     INTEGER NULL REFERENCES external_companies(id),
                status_code             TEXT    NOT NULL DEFAULT 'APPROVED',
                payment_terms_code      TEXT    NULL,
                currency_value_id       INTEGER NULL,
                incoterms_code          TEXT    NULL,
                default_lead_time_days  INTEGER NULL,
                default_buyer_person_id INTEGER NULL,
                is_active               INTEGER NOT NULL DEFAULT 1,
                row_version             INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_supplier_article_sources (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                supplier_id             INTEGER NOT NULL REFERENCES inventory_suppliers(id),
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                is_preferred            INTEGER NOT NULL DEFAULT 0,
                priority                INTEGER NOT NULL DEFAULT 100,
                lead_time_days          INTEGER NULL,
                unit_price_hint         REAL    NULL,
                min_order_qty           REAL    NULL,
                supplier_article_code   TEXT    NULL,
                is_active               INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                UNIQUE(supplier_id, article_id)
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_supplier_prices (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                supplier_id         INTEGER NOT NULL REFERENCES inventory_suppliers(id),
                article_id          INTEGER NOT NULL REFERENCES articles(id),
                unit_price          REAL    NOT NULL,
                currency_value_id   INTEGER NULL,
                price_unit_value_id INTEGER NULL,
                min_order_qty       REAL    NULL,
                valid_from          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                valid_to            TEXT    NULL,
                is_active           INTEGER NOT NULL DEFAULT 1,
                created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_article_equivalents (
                id                      INTEGER PRIMARY KEY AUTOINCREMENT,
                article_id              INTEGER NOT NULL REFERENCES articles(id),
                equivalent_article_id   INTEGER NOT NULL REFERENCES articles(id),
                equivalence_type        TEXT    NOT NULL DEFAULT 'DIRECT',
                notes                   TEXT    NULL,
                is_bidirectional        INTEGER NOT NULL DEFAULT 1,
                created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                CHECK (article_id <> equivalent_article_id),
                UNIQUE(article_id, equivalent_article_id)
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS inventory_document_links (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_type     TEXT    NOT NULL,
                entity_id       INTEGER NOT NULL,
                document_ref    TEXT    NOT NULL,
                link_purpose    TEXT    NOT NULL,
                is_primary      INTEGER NOT NULL DEFAULT 0,
                valid_from      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                valid_to        TEXT    NULL,
                created_by_id   INTEGER NULL,
                created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE INDEX IF NOT EXISTS idx_inventory_document_links_entity
             ON inventory_document_links(entity_type, entity_id)"
                .to_string(),
        ))
        .await?;

        // --- PO supplier_id (dual-write with legacy supplier_company_id) ---
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE purchase_orders ADD COLUMN supplier_id INTEGER NULL REFERENCES inventory_suppliers(id)"
                .to_string(),
        ))
        .await?;

        // --- Requisition purchase priority + demand line ---
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE procurement_requisitions ADD COLUMN purchase_priority TEXT NOT NULL DEFAULT 'NORMAL'"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE procurement_requisition_lines ADD COLUMN demand_source_line_id INTEGER NULL"
                .to_string(),
        ))
        .await?;

        // --- GR delivery accuracy / lead time ---
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE goods_receipt_lines ADD COLUMN ordered_qty REAL NULL".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE goods_receipt_lines ADD COLUMN actual_lead_time_days REAL NULL".to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "ALTER TABLE purchase_order_lines ADD COLUMN demand_source_line_id INTEGER NULL".to_string(),
        ))
        .await?;

        // --- Article identity + replenishment + critical spare + shelf life ---
        for sql in [
            "ALTER TABLE articles ADD COLUMN manufacturer_name TEXT NULL",
            "ALTER TABLE articles ADD COLUMN manufacturer_part_number TEXT NULL",
            "ALTER TABLE articles ADD COLUMN oem_part_number TEXT NULL",
            "ALTER TABLE articles ADD COLUMN replenishment_policy_code TEXT NULL DEFAULT 'MIN_MAX'",
            "ALTER TABLE articles ADD COLUMN economic_order_qty REAL NULL",
            "ALTER TABLE articles ADD COLUMN minimum_order_qty REAL NULL",
            "ALTER TABLE articles ADD COLUMN maximum_order_qty REAL NULL",
            "ALTER TABLE articles ADD COLUMN order_multiple_qty REAL NULL",
            "ALTER TABLE articles ADD COLUMN lead_time_days INTEGER NULL",
            "ALTER TABLE articles ADD COLUMN review_period_days INTEGER NULL",
            "ALTER TABLE articles ADD COLUMN abc_class_code TEXT NULL",
            "ALTER TABLE articles ADD COLUMN xyz_class_code TEXT NULL",
            "ALTER TABLE articles ADD COLUMN is_critical_spare INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE articles ADD COLUMN requires_expiration INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE articles ADD COLUMN shelf_life_days INTEGER NULL",
            "ALTER TABLE articles ADD COLUMN requires_batch_tracking INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE repairable_orders ADD COLUMN repair_cost REAL NULL",
        ] {
            db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                .await?;
        }

        // Seed suppliers from existing external_companies used on POs (or all active companies)
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO inventory_suppliers (code, name, external_company_id, status_code, is_active)
             SELECT printf('SUP-%05d', ec.id), ec.name, ec.id, 'APPROVED', COALESCE(ec.is_active, 1)
             FROM external_companies ec"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "UPDATE purchase_orders
             SET supplier_id = (
               SELECT s.id FROM inventory_suppliers s
               WHERE s.external_company_id = purchase_orders.supplier_company_id
             )
             WHERE supplier_company_id IS NOT NULL AND supplier_id IS NULL"
                .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO app_settings
             (setting_key, category, setting_scope, setting_value_json, setting_risk, validation_status, last_modified_at)
             VALUES
             ('procurement.abc_a_pct', 'procurement', 'tenant', '80', 'low', 'valid',
              strftime('%Y-%m-%dT%H:%M:%SZ','now')),
             ('procurement.abc_b_pct', 'procurement', 'tenant', '95', 'low', 'valid',
              strftime('%Y-%m-%dT%H:%M:%SZ','now')),
             ('procurement.xyz_x_cv_max', 'procurement', 'tenant', '0.5', 'low', 'valid',
              strftime('%Y-%m-%dT%H:%M:%SZ','now')),
             ('procurement.xyz_y_cv_max', 'procurement', 'tenant', '1.0', 'low', 'valid',
              strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
                .to_string(),
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let _ = manager;
        Ok(())
    }
}

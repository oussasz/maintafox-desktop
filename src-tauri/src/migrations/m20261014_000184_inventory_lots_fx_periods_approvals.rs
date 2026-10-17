//! Migration 184 — inventory benchmark gaps (lots, currencies, valuation periods, approvals).
//!
//! Additive only; every new table is synchronised (generic row capture) and every new column of an
//! existing synchronised table travels with its row.
//!
//! - Articles: `barcode` (scan at the issue desk) and `requires_serial_tracking` (one lot row per
//!   serial number). `requires_batch_tracking` / `requires_expiration` already exist.
//! - `inventory_lots`: lot or serial of a tracked article, with its expiry date and the receipt
//!   line it came in on. Quantities are not stored: a lot's stock at a location is the sum of the
//!   ledger movements that carry its `lot_id` (`inventory_transactions.lot_id`), so it can never
//!   drift from the ledger and needs no projection on receiving devices.
//! - `inventory_transactions.posting_date`: business date of a back-dated posting (receipt or
//!   adjustment entered after the fact). NULL means the date of `performed_at`.
//! - Currencies: `inventory_suppliers.currency_code`, `purchase_orders.currency_code` (the PO is
//!   priced in the supplier currency; NULL = tenant currency), the receipt's `currency_code`,
//!   `exchange_rate` (1 PO currency unit = rate tenant units) and where the rate came from, and
//!   per receipt line the PO price and the resulting tenant-currency unit cost.
//!   `currency_exchange_rates` stores dated rates entered by the user; no rate is built in.
//! - `inventory_valuation_periods` + `inventory_valuation_snapshot_lines`: closing a period
//!   freezes the stock valuation at its end date (append-only snapshot lines).
//! - `procurement_approval_limits`: per role and document type, the largest amount (tenant
//!   currency) a holder of the role may approve; NULL = unlimited.
//!
//! No unique indexes on business keys (period code, lot number, rate date): two devices may create
//! the same key offline and a unique index would block the second one from arriving. The services
//! refuse duplicates on one device.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261014_000184_inventory_lots_fx_periods_approvals"
    }
}

const STATEMENTS: &[&str] = &[
    // ── Articles ────────────────────────────────────────────────────────────
    "ALTER TABLE articles ADD COLUMN barcode TEXT NULL",
    "ALTER TABLE articles ADD COLUMN requires_serial_tracking INTEGER NOT NULL DEFAULT 0",
    "CREATE INDEX IF NOT EXISTS idx_articles_barcode ON articles(barcode)",
    // ── Lots / serials ─────────────────────────────────────────────────────
    "CREATE TABLE IF NOT EXISTS inventory_lots (
        id                    INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id        TEXT NOT NULL UNIQUE,
        article_id            INTEGER NOT NULL REFERENCES articles(id),
        lot_number            TEXT NULL,
        serial_number         TEXT NULL,
        expiry_date           TEXT NULL,
        goods_receipt_line_id INTEGER NULL REFERENCES goods_receipt_lines(id),
        notes                 TEXT NULL,
        row_version           INTEGER NOT NULL DEFAULT 1,
        created_by_id         INTEGER NULL REFERENCES user_accounts(id),
        created_at            TEXT NOT NULL,
        updated_at            TEXT NOT NULL,
        CHECK (lot_number IS NOT NULL OR serial_number IS NOT NULL)
    )",
    "CREATE INDEX IF NOT EXISTS idx_inventory_lots_article ON inventory_lots(article_id)",
    "CREATE INDEX IF NOT EXISTS idx_inventory_lots_expiry ON inventory_lots(expiry_date)",
    "ALTER TABLE inventory_transactions ADD COLUMN lot_id INTEGER NULL REFERENCES inventory_lots(id)",
    "ALTER TABLE inventory_transactions ADD COLUMN posting_date TEXT NULL",
    "CREATE INDEX IF NOT EXISTS idx_inventory_transactions_lot ON inventory_transactions(lot_id)",
    // ── Currencies ─────────────────────────────────────────────────────────
    "ALTER TABLE inventory_suppliers ADD COLUMN currency_code TEXT NULL",
    "ALTER TABLE purchase_orders ADD COLUMN currency_code TEXT NULL",
    "ALTER TABLE goods_receipts ADD COLUMN currency_code TEXT NULL",
    "ALTER TABLE goods_receipts ADD COLUMN exchange_rate REAL NULL",
    "ALTER TABLE goods_receipts ADD COLUMN exchange_rate_source TEXT NULL",
    "ALTER TABLE goods_receipts ADD COLUMN posting_date TEXT NULL",
    "ALTER TABLE goods_receipt_lines ADD COLUMN unit_price REAL NULL",
    "ALTER TABLE goods_receipt_lines ADD COLUMN unit_cost_tenant REAL NULL",
    "CREATE TABLE IF NOT EXISTS currency_exchange_rates (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id TEXT NOT NULL UNIQUE,
        from_currency  TEXT NOT NULL,
        to_currency    TEXT NOT NULL,
        rate           REAL NOT NULL CHECK (rate > 0),
        valid_from     TEXT NOT NULL,
        source         TEXT NULL,
        is_active      INTEGER NOT NULL DEFAULT 1,
        row_version    INTEGER NOT NULL DEFAULT 1,
        created_by_id  INTEGER NULL REFERENCES user_accounts(id),
        created_at     TEXT NOT NULL,
        updated_at     TEXT NOT NULL,
        CHECK (from_currency <> to_currency)
    )",
    "CREATE INDEX IF NOT EXISTS idx_currency_exchange_rates_pair \
     ON currency_exchange_rates(from_currency, to_currency, valid_from)",
    // ── Valuation periods ──────────────────────────────────────────────────
    "CREATE TABLE IF NOT EXISTS inventory_valuation_periods (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id TEXT NOT NULL UNIQUE,
        period_code    TEXT NOT NULL,
        period_start   TEXT NOT NULL,
        period_end     TEXT NOT NULL,
        status         TEXT NOT NULL DEFAULT 'closed' CHECK (status IN ('closed')),
        currency_code  TEXT NOT NULL,
        total_value    REAL NOT NULL DEFAULT 0,
        unvalued_qty   REAL NOT NULL DEFAULT 0,
        notes          TEXT NULL,
        closed_by_id   INTEGER NULL REFERENCES user_accounts(id),
        closed_at      TEXT NOT NULL,
        row_version    INTEGER NOT NULL DEFAULT 1,
        created_at     TEXT NOT NULL,
        updated_at     TEXT NOT NULL,
        CHECK (period_start <= period_end)
    )",
    "CREATE INDEX IF NOT EXISTS idx_inventory_valuation_periods_end ON inventory_valuation_periods(period_end)",
    "CREATE TABLE IF NOT EXISTS inventory_valuation_snapshot_lines (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id TEXT NOT NULL UNIQUE,
        period_id      INTEGER NOT NULL REFERENCES inventory_valuation_periods(id),
        article_id     INTEGER NOT NULL REFERENCES articles(id),
        warehouse_id   INTEGER NOT NULL REFERENCES warehouses(id),
        quantity       REAL NOT NULL,
        unit_cost      REAL NULL,
        value          REAL NULL,
        unvalued_qty   REAL NOT NULL DEFAULT 0,
        created_at     TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_inventory_valuation_snapshot_period \
     ON inventory_valuation_snapshot_lines(period_id)",
    // ── Approval limits ────────────────────────────────────────────────────
    "CREATE TABLE IF NOT EXISTS procurement_approval_limits (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        entity_sync_id TEXT NOT NULL UNIQUE,
        role_id        INTEGER NOT NULL REFERENCES roles(id),
        document_type  TEXT NOT NULL CHECK (document_type IN ('REQUISITION', 'PURCHASE_ORDER')),
        max_amount     REAL NULL CHECK (max_amount IS NULL OR max_amount >= 0),
        is_active      INTEGER NOT NULL DEFAULT 1,
        row_version    INTEGER NOT NULL DEFAULT 1,
        created_by_id  INTEGER NULL REFERENCES user_accounts(id),
        created_at     TEXT NOT NULL,
        updated_at     TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_procurement_approval_limits_role \
     ON procurement_approval_limits(role_id, document_type)",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for statement in STATEMENTS {
            db.execute_unprepared(statement).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Columns added to existing tables stay (SQLite cannot drop them safely in place);
        // the new tables are removed.
        let db = manager.get_connection();
        for table in [
            "procurement_approval_limits",
            "inventory_valuation_snapshot_lines",
            "inventory_valuation_periods",
            "currency_exchange_rates",
        ] {
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {table}")).await?;
        }
        Ok(())
    }
}

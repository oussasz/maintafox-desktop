//! Inventory supplier master, article sourcing, price lists, and scorecard.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    InventorySupplier, InventorySupplierInput, SupplierArticleSource, SupplierArticleSourceInput,
    SupplierContact, SupplierContactInput, SupplierPrice, SupplierPriceInput,
    SupplierPurchaseHistoryRow, SupplierRecentDelivery, SupplierScorecard,
};

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ── Internal helpers ────────────────────────────────────────────────────────

async fn get_supplier<C: ConnectionTrait>(db: &C, supplier_id: i64) -> AppResult<InventorySupplier> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, external_company_id, status_code, payment_terms_code,
                    currency_value_id, incoterms_code, default_lead_time_days, default_buyer_person_id,
                    is_active, row_version, created_at, updated_at
             FROM inventory_suppliers WHERE id = ?",
            [supplier_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_suppliers".to_string(),
            id: supplier_id.to_string(),
        })?;
    map_supplier_row(row)
}

fn map_supplier_row(row: sea_orm::QueryResult) -> AppResult<InventorySupplier> {
    Ok(InventorySupplier {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        external_company_id: row.try_get("", "external_company_id")?,
        status_code: row.try_get("", "status_code")?,
        payment_terms_code: row.try_get("", "payment_terms_code")?,
        currency_value_id: row.try_get("", "currency_value_id")?,
        incoterms_code: row.try_get("", "incoterms_code")?,
        default_lead_time_days: row.try_get("", "default_lead_time_days")?,
        default_buyer_person_id: row.try_get("", "default_buyer_person_id")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_source_row(row: sea_orm::QueryResult) -> AppResult<SupplierArticleSource> {
    Ok(SupplierArticleSource {
        id: row.try_get("", "id")?,
        supplier_id: row.try_get("", "supplier_id")?,
        supplier_code: row.try_get("", "supplier_code")?,
        supplier_name: row.try_get("", "supplier_name")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        is_preferred: row.try_get("", "is_preferred")?,
        priority: row.try_get("", "priority")?,
        lead_time_days: row.try_get("", "lead_time_days")?,
        unit_price_hint: row.try_get("", "unit_price_hint")?,
        min_order_qty: row.try_get("", "min_order_qty")?,
        supplier_article_code: row.try_get("", "supplier_article_code")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
        // Enrichment fields are filled by list queries that join purchase history; base mapping leaves them empty.
        last_price: row.try_get("", "last_price").ok().flatten(),
        avg_price: row.try_get("", "avg_price").ok().flatten(),
        last_purchase_at: row.try_get("", "last_purchase_at").ok().flatten(),
        currency_value_id: row.try_get("", "currency_value_id").ok().flatten(),
        currency_label: row.try_get("", "currency_label").ok().flatten(),
        risk_level: row.try_get("", "risk_level").ok().flatten(),
    })
}

/// Classifies a supplier as LOW / MEDIUM / HIGH from delivery metrics (KPI-020).
/// Returns `None` when there is no receipt evidence — callers treat that as LOW by absence of evidence.
pub(crate) fn compute_supplier_risk_level(
    on_time_delivery_pct: Option<f64>,
    rejected_pct: Option<f64>,
    avg_lead_time_days: Option<f64>,
    promised_lead_time_days: Option<i64>,
) -> Option<String> {
    if on_time_delivery_pct.is_none() && rejected_pct.is_none() && avg_lead_time_days.is_none() {
        return None;
    }

    let mut score = 0u8;
    if on_time_delivery_pct.is_some_and(|pct| pct < 70.0) {
        score += 2;
    } else if on_time_delivery_pct.is_some_and(|pct| pct < 90.0) {
        score += 1;
    }
    if rejected_pct.is_some_and(|pct| pct >= 10.0) {
        score += 2;
    } else if rejected_pct.is_some_and(|pct| pct >= 3.0) {
        score += 1;
    }
    if let (Some(actual), Some(promised)) = (avg_lead_time_days, promised_lead_time_days) {
        if promised > 0 {
            let overrun = (actual - promised as f64) / promised as f64;
            if overrun >= 0.5 {
                score += 2;
            } else if overrun >= 0.2 {
                score += 1;
            }
        }
    }

    Some(
        if score >= 4 {
            "HIGH"
        } else if score >= 2 {
            "MEDIUM"
        } else {
            "LOW"
        }
        .to_string(),
    )
}

fn map_price_row(row: sea_orm::QueryResult) -> AppResult<SupplierPrice> {
    Ok(SupplierPrice {
        id: row.try_get("", "id")?,
        supplier_id: row.try_get("", "supplier_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        unit_price: row.try_get("", "unit_price")?,
        currency_value_id: row.try_get("", "currency_value_id")?,
        price_unit_value_id: row.try_get("", "price_unit_value_id")?,
        min_order_qty: row.try_get("", "min_order_qty")?,
        valid_from: row.try_get("", "valid_from")?,
        valid_to: row.try_get("", "valid_to")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

// ── Supplier CRUD ────────────────────────────────────────────────────────────

pub async fn list_suppliers(db: &DatabaseConnection) -> AppResult<Vec<InventorySupplier>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, external_company_id, status_code, payment_terms_code,
                    currency_value_id, incoterms_code, default_lead_time_days, default_buyer_person_id,
                    is_active, row_version, created_at, updated_at
             FROM inventory_suppliers
             ORDER BY name ASC"
                .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_supplier_row).collect()
}

pub async fn get_supplier_by_id(db: &DatabaseConnection, supplier_id: i64) -> AppResult<InventorySupplier> {
    get_supplier(db, supplier_id).await
}

pub async fn upsert_supplier(
    db: &DatabaseConnection,
    supplier_id: Option<i64>,
    expected_row_version: Option<i64>,
    input: InventorySupplierInput,
) -> AppResult<InventorySupplier> {
    if input.code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["Supplier code is required.".to_string()]));
    }
    if input.name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["Supplier name is required.".to_string()]));
    }
    let allowed_statuses = ["PREFERRED", "APPROVED", "BLOCKED", "UNDER_EVALUATION"];
    if !allowed_statuses.contains(&input.status_code.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Invalid status_code '{}'. Must be one of: PREFERRED, APPROVED, BLOCKED, UNDER_EVALUATION.",
            input.status_code
        )]));
    }
    let is_active = input.is_active.unwrap_or(true) as i64;

    if let Some(id) = supplier_id {
        let expected_rv = expected_row_version.ok_or_else(|| {
            AppError::ValidationFailed(vec!["expected_row_version is required for updates.".to_string()])
        })?;
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT row_version FROM inventory_suppliers WHERE id = ?",
                [id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "inventory_suppliers".to_string(),
                id: id.to_string(),
            })?;
        let current_rv: i64 = row.try_get("", "row_version")?;
        if current_rv != expected_rv {
            return Err(AppError::ValidationFailed(vec!["Supplier row_version mismatch.".to_string()]));
        }
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_suppliers
             SET code = ?, name = ?, external_company_id = ?, status_code = ?, payment_terms_code = ?,
                 currency_value_id = ?, incoterms_code = ?, default_lead_time_days = ?,
                 default_buyer_person_id = ?, is_active = ?, row_version = row_version + 1, updated_at = ?
             WHERE id = ?",
            [
                input.code.into(),
                input.name.into(),
                input.external_company_id.map_or(Value::BigInt(None), Value::from),
                input.status_code.into(),
                input.payment_terms_code.map_or(Value::String(None), Value::from),
                input.currency_value_id.map_or(Value::BigInt(None), Value::from),
                input.incoterms_code.map_or(Value::String(None), Value::from),
                input.default_lead_time_days.map_or(Value::BigInt(None), Value::from),
                input.default_buyer_person_id.map_or(Value::BigInt(None), Value::from),
                is_active.into(),
                now_iso().into(),
                id.into(),
            ],
        ))
        .await?;
        get_supplier(db, id).await
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_suppliers
                (code, name, external_company_id, status_code, payment_terms_code, currency_value_id,
                 incoterms_code, default_lead_time_days, default_buyer_person_id, is_active, row_version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            [
                input.code.into(),
                input.name.into(),
                input.external_company_id.map_or(Value::BigInt(None), Value::from),
                input.status_code.into(),
                input.payment_terms_code.map_or(Value::String(None), Value::from),
                input.currency_value_id.map_or(Value::BigInt(None), Value::from),
                input.incoterms_code.map_or(Value::String(None), Value::from),
                input.default_lead_time_days.map_or(Value::BigInt(None), Value::from),
                input.default_buyer_person_id.map_or(Value::BigInt(None), Value::from),
                is_active.into(),
                now_iso().into(),
                now_iso().into(),
            ],
        ))
        .await?;
        let id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to create supplier.".to_string()]))?
            .try_get("", "id")?;
        get_supplier(db, id).await
    }
}

pub async fn soft_delete_supplier(
    db: &DatabaseConnection,
    supplier_id: i64,
    expected_row_version: i64,
) -> AppResult<InventorySupplier> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM inventory_suppliers WHERE id = ?",
            [supplier_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_suppliers".to_string(),
            id: supplier_id.to_string(),
        })?;
    let current_rv: i64 = row.try_get("", "row_version")?;
    if current_rv != expected_row_version {
        return Err(AppError::ValidationFailed(vec!["Supplier row_version mismatch.".to_string()]));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_suppliers SET is_active = 0, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [now_iso().into(), supplier_id.into()],
    ))
    .await?;
    get_supplier(db, supplier_id).await
}

// ── Article sources ──────────────────────────────────────────────────────────

pub async fn list_supplier_article_sources(
    db: &DatabaseConnection,
    supplier_id: Option<i64>,
    article_id: Option<i64>,
) -> AppResult<Vec<SupplierArticleSource>> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();
    if let Some(sid) = supplier_id {
        conditions.push("sas.supplier_id = ?".to_string());
        params.push(sid.into());
    }
    if let Some(aid) = article_id {
        conditions.push("sas.article_id = ?".to_string());
        params.push(aid.into());
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT sas.id, sas.supplier_id, s.code AS supplier_code, s.name AS supplier_name,
                sas.article_id, a.article_code, a.article_name,
                sas.is_preferred, sas.priority, sas.lead_time_days, sas.unit_price_hint,
                sas.min_order_qty, sas.supplier_article_code, sas.is_active, sas.created_at, sas.updated_at
         FROM inventory_supplier_article_sources sas
         JOIN inventory_suppliers s ON s.id = sas.supplier_id
         JOIN articles a ON a.id = sas.article_id
         {where_clause}
         ORDER BY sas.is_preferred DESC, sas.priority ASC, s.name ASC"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, params))
        .await?;
    rows.into_iter().map(map_source_row).collect()
}

pub async fn upsert_supplier_article_source(
    db: &DatabaseConnection,
    input: SupplierArticleSourceInput,
) -> AppResult<SupplierArticleSource> {
    // Validate supplier exists and is not BLOCKED
    let sup_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status_code, is_active FROM inventory_suppliers WHERE id = ?",
            [input.supplier_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_suppliers".to_string(),
            id: input.supplier_id.to_string(),
        })?;
    let is_active: i64 = sup_row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec!["Supplier is inactive.".to_string()]));
    }
    // Validate article exists
    let art_exists: bool = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM articles WHERE id = ?",
            [input.article_id.into()],
        ))
        .await?
        .is_some();
    if !art_exists {
        return Err(AppError::NotFound {
            entity: "articles".to_string(),
            id: input.article_id.to_string(),
        });
    }

    let is_preferred = input.is_preferred.unwrap_or(false) as i64;
    let priority = input.priority.unwrap_or(100);
    let src_is_active = input.is_active.unwrap_or(true) as i64;

    if is_preferred == 1 {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_supplier_article_sources
             SET is_preferred = 0, updated_at = ?
             WHERE article_id = ? AND supplier_id <> ?",
            [now_iso().into(), input.article_id.into(), input.supplier_id.into()],
        ))
        .await?;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_supplier_article_sources
            (supplier_id, article_id, is_preferred, priority, lead_time_days, unit_price_hint,
             min_order_qty, supplier_article_code, is_active, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(supplier_id, article_id) DO UPDATE SET
            is_preferred = excluded.is_preferred,
            priority = excluded.priority,
            lead_time_days = excluded.lead_time_days,
            unit_price_hint = excluded.unit_price_hint,
            min_order_qty = excluded.min_order_qty,
            supplier_article_code = excluded.supplier_article_code,
            is_active = excluded.is_active,
            updated_at = excluded.updated_at",
        [
            input.supplier_id.into(),
            input.article_id.into(),
            is_preferred.into(),
            priority.into(),
            input.lead_time_days.map_or(Value::BigInt(None), Value::from),
            input.unit_price_hint.map_or(Value::Double(None), Value::from),
            input.min_order_qty.map_or(Value::Double(None), Value::from),
            input.supplier_article_code.clone().map_or(Value::String(None), Value::from),
            src_is_active.into(),
            now_iso().into(),
            now_iso().into(),
        ],
    ))
    .await?;

    let rows = list_supplier_article_sources(db, Some(input.supplier_id), Some(input.article_id)).await?;
    rows.into_iter().next().ok_or_else(|| AppError::ValidationFailed(vec!["Failed to retrieve article source.".to_string()]))
}

pub async fn delete_supplier_article_source(
    db: &DatabaseConnection,
    source_id: i64,
) -> AppResult<()> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inventory_supplier_article_sources WHERE id = ?",
            [source_id.into()],
        ))
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound {
            entity: "inventory_supplier_article_sources".to_string(),
            id: source_id.to_string(),
        });
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM inventory_supplier_article_sources WHERE id = ?",
        [source_id.into()],
    ))
    .await?;
    Ok(())
}

// ── Supplier prices ──────────────────────────────────────────────────────────

pub async fn list_supplier_prices(
    db: &DatabaseConnection,
    supplier_id: i64,
    article_id: Option<i64>,
) -> AppResult<Vec<SupplierPrice>> {
    let mut conditions = vec!["sp.supplier_id = ?".to_string()];
    let mut params: Vec<Value> = vec![supplier_id.into()];
    if let Some(aid) = article_id {
        conditions.push("sp.article_id = ?".to_string());
        params.push(aid.into());
    }
    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT sp.id, sp.supplier_id, sp.article_id, a.article_code, a.article_name,
                sp.unit_price, sp.currency_value_id, sp.price_unit_value_id, sp.min_order_qty,
                sp.valid_from, sp.valid_to, sp.is_active, sp.created_at, sp.updated_at
         FROM inventory_supplier_prices sp
         JOIN articles a ON a.id = sp.article_id
         WHERE {where_clause}
         ORDER BY sp.valid_from DESC"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, params))
        .await?;
    rows.into_iter().map(map_price_row).collect()
}

pub async fn upsert_supplier_price(
    db: &DatabaseConnection,
    price_id: Option<i64>,
    input: SupplierPriceInput,
) -> AppResult<SupplierPrice> {
    if input.unit_price < 0.0 {
        return Err(AppError::ValidationFailed(vec!["unit_price cannot be negative.".to_string()]));
    }
    let sup_exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inventory_suppliers WHERE id = ? AND is_active = 1",
            [input.supplier_id.into()],
        ))
        .await?;
    if sup_exists.is_none() {
        return Err(AppError::NotFound {
            entity: "inventory_suppliers".to_string(),
            id: input.supplier_id.to_string(),
        });
    }
    let art_exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM articles WHERE id = ?",
            [input.article_id.into()],
        ))
        .await?;
    if art_exists.is_none() {
        return Err(AppError::NotFound {
            entity: "articles".to_string(),
            id: input.article_id.to_string(),
        });
    }
    let valid_from = input.valid_from.clone().unwrap_or_else(now_iso);
    let price_is_active = input.is_active.unwrap_or(true) as i64;

    let final_id = if let Some(id) = price_id {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_supplier_prices
             SET unit_price = ?, currency_value_id = ?, price_unit_value_id = ?, min_order_qty = ?,
                 valid_from = ?, valid_to = ?, is_active = ?, updated_at = ?
             WHERE id = ?",
            [
                input.unit_price.into(),
                input.currency_value_id.map_or(Value::BigInt(None), Value::from),
                input.price_unit_value_id.map_or(Value::BigInt(None), Value::from),
                input.min_order_qty.map_or(Value::Double(None), Value::from),
                valid_from.into(),
                input.valid_to.clone().map_or(Value::String(None), Value::from),
                price_is_active.into(),
                now_iso().into(),
                id.into(),
            ],
        ))
        .await?;
        id
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_supplier_prices
                (supplier_id, article_id, unit_price, currency_value_id, price_unit_value_id,
                 min_order_qty, valid_from, valid_to, is_active, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                input.supplier_id.into(),
                input.article_id.into(),
                input.unit_price.into(),
                input.currency_value_id.map_or(Value::BigInt(None), Value::from),
                input.price_unit_value_id.map_or(Value::BigInt(None), Value::from),
                input.min_order_qty.map_or(Value::Double(None), Value::from),
                valid_from.into(),
                input.valid_to.clone().map_or(Value::String(None), Value::from),
                price_is_active.into(),
                now_iso().into(),
                now_iso().into(),
            ],
        ))
        .await?;
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to create price.".to_string()]))?
        .try_get("", "id")?
    };

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sp.id, sp.supplier_id, sp.article_id, a.article_code, a.article_name,
                    sp.unit_price, sp.currency_value_id, sp.price_unit_value_id, sp.min_order_qty,
                    sp.valid_from, sp.valid_to, sp.is_active, sp.created_at, sp.updated_at
             FROM inventory_supplier_prices sp
             JOIN articles a ON a.id = sp.article_id
             WHERE sp.id = ?",
            [final_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_supplier_prices".to_string(),
            id: final_id.to_string(),
        })?;
    map_price_row(row)
}

/// Returns the best active price for a supplier+article pair as of now.
pub async fn resolve_effective_price(
    db: &DatabaseConnection,
    supplier_id: i64,
    article_id: i64,
) -> AppResult<Option<f64>> {
    let now = now_iso();
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_price
             FROM inventory_supplier_prices
             WHERE supplier_id = ? AND article_id = ? AND is_active = 1
               AND valid_from <= ?
               AND (valid_to IS NULL OR valid_to >= ?)
             ORDER BY valid_from DESC
             LIMIT 1",
            [supplier_id.into(), article_id.into(), now.clone().into(), now.into()],
        ))
        .await?;
    if let Some(row) = row {
        let price: f64 = row.try_get("", "unit_price")?;
        return Ok(Some(price));
    }
    // Fallback: unit_price_hint from article sources
    let hint_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_price_hint
             FROM inventory_supplier_article_sources
             WHERE supplier_id = ? AND article_id = ? AND is_active = 1 AND unit_price_hint IS NOT NULL
             ORDER BY is_preferred DESC, priority ASC
             LIMIT 1",
            [supplier_id.into(), article_id.into()],
        ))
        .await?;
    if let Some(row) = hint_row {
        let hint: Option<f64> = row.try_get("", "unit_price_hint").ok();
        return Ok(hint);
    }
    Ok(None)
}

// ── Scorecard ────────────────────────────────────────────────────────────────

pub async fn get_supplier_scorecard(
    db: &DatabaseConnection,
    supplier_id: i64,
) -> AppResult<SupplierScorecard> {
    let sup = get_supplier(db, supplier_id).await?;

    let accuracy_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                COUNT(*) AS total_lines,
                SUM(CASE WHEN grl.accepted_qty >= COALESCE(grl.ordered_qty, pol.ordered_qty) THEN 1 ELSE 0 END) AS accurate_lines,
                AVG(grl.actual_lead_time_days) AS avg_lead_time,
                SUM(COALESCE(grl.accepted_qty, 0.0) + COALESCE(grl.rejected_qty, 0.0)) AS total_received,
                SUM(COALESCE(grl.rejected_qty, 0.0)) AS total_rejected,
                SUM(CASE
                      WHEN po.expected_delivery_date IS NOT NULL
                       AND date(COALESCE(gr.received_at, grl.created_at)) <= date(po.expected_delivery_date)
                       AND grl.accepted_qty >= COALESCE(grl.ordered_qty, pol.ordered_qty)
                      THEN 1
                      WHEN po.expected_delivery_date IS NULL
                       AND grl.accepted_qty >= COALESCE(grl.ordered_qty, pol.ordered_qty)
                      THEN 1
                      ELSE 0
                    END) AS on_time_lines
             FROM goods_receipt_lines grl
             JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             LEFT JOIN goods_receipts gr ON gr.id = grl.goods_receipt_id
             WHERE po.supplier_id = ?",
            [supplier_id.into()],
        ))
        .await?;

    let (on_time_delivery_pct, avg_lead_time_days, delivery_accuracy_pct, rejected_pct) =
        if let Some(row) = accuracy_row {
            let total: i64 = row.try_get("", "total_lines").unwrap_or(0);
            let accurate: i64 = row.try_get("", "accurate_lines").unwrap_or(0);
            let on_time: i64 = row.try_get("", "on_time_lines").unwrap_or(0);
            let avg_lt: Option<f64> = row.try_get("", "avg_lead_time").ok().flatten();
            let total_recv: f64 = row.try_get("", "total_received").unwrap_or(0.0);
            let total_rej: f64 = row.try_get("", "total_rejected").unwrap_or(0.0);
            let acc_pct = if total > 0 {
                Some(accurate as f64 / total as f64 * 100.0)
            } else {
                None
            };
            let otif = if total > 0 {
                Some(on_time as f64 / total as f64 * 100.0)
            } else {
                None
            };
            let rej = if total_recv > 0.0 {
                Some(total_rej / total_recv * 100.0)
            } else {
                None
            };
            (otif, avg_lt, acc_pct, rej)
        } else {
            (None, None, None, None)
        };

    let price_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT AVG(pol.unit_price) AS avg_price
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             WHERE po.supplier_id = ? AND pol.unit_price IS NOT NULL AND pol.status = 'CLOSED'",
            [supplier_id.into()],
        ))
        .await?;
    let avg_price: Option<f64> = price_row
        .and_then(|r| r.try_get("", "avg_price").ok())
        .flatten();

    let counts_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                SUM(CASE WHEN po.status NOT IN ('RECEIVED_CLOSED', 'CANCELLED') THEN 1 ELSE 0 END) AS open_count,
                SUM(CASE WHEN po.status = 'RECEIVED_CLOSED' THEN 1 ELSE 0 END) AS completed_count,
                MAX(po.ordered_at) AS last_purchase_at
             FROM purchase_orders po
             WHERE po.supplier_id = ?",
            [supplier_id.into()],
        ))
        .await?;
    let (open_po_count, completed_po_count, last_purchase_at) = if let Some(row) = counts_row {
        let open: i64 = row.try_get("", "open_count").unwrap_or(0);
        let completed: i64 = row.try_get("", "completed_count").unwrap_or(0);
        let last: Option<String> = row.try_get("", "last_purchase_at").ok().flatten();
        (open, completed, last)
    } else {
        (0, 0, None)
    };

    let delivery_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(gr.received_at, grl.created_at) AS received_at,
                    po.po_number, a.article_code, a.article_name,
                    COALESCE(grl.ordered_qty, pol.ordered_qty) AS ordered_qty,
                    grl.accepted_qty, COALESCE(grl.rejected_qty, 0) AS rejected_qty,
                    grl.actual_lead_time_days
             FROM goods_receipt_lines grl
             JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             JOIN articles a ON a.id = grl.article_id
             LEFT JOIN goods_receipts gr ON gr.id = grl.goods_receipt_id
             WHERE po.supplier_id = ?
             ORDER BY COALESCE(gr.received_at, grl.created_at) DESC
             LIMIT 10",
            [supplier_id.into()],
        ))
        .await
        .unwrap_or_default();

    let last_deliveries: Vec<SupplierRecentDelivery> = delivery_rows
        .into_iter()
        .filter_map(|row| {
            Some(SupplierRecentDelivery {
                received_at: row.try_get("", "received_at").ok().flatten(),
                po_number: row.try_get("", "po_number").ok()?,
                article_code: row.try_get("", "article_code").ok()?,
                article_name: row.try_get("", "article_name").ok()?,
                ordered_qty: row.try_get("", "ordered_qty").ok().flatten(),
                accepted_qty: row.try_get("", "accepted_qty").unwrap_or(0.0),
                rejected_qty: row.try_get("", "rejected_qty").unwrap_or(0.0),
                actual_lead_time_days: row.try_get("", "actual_lead_time_days").ok().flatten(),
            })
        })
        .collect();

    let risk_level = compute_supplier_risk_level(
        on_time_delivery_pct,
        rejected_pct,
        avg_lead_time_days,
        sup.default_lead_time_days,
    )
    .unwrap_or_else(|| "LOW".to_string());

    Ok(SupplierScorecard {
        supplier_id,
        supplier_code: sup.code,
        supplier_name: sup.name,
        on_time_delivery_pct,
        avg_lead_time_days,
        delivery_accuracy_pct,
        avg_price,
        open_po_count,
        completed_po_count,
        last_purchase_at,
        rejected_pct,
        risk_level,
        last_deliveries,
    })
}

// ── Contacts ─────────────────────────────────────────────────────────────────

pub async fn list_supplier_contacts(
    db: &DatabaseConnection,
    supplier_id: i64,
) -> AppResult<Vec<SupplierContact>> {
    let _ = get_supplier(db, supplier_id).await?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, supplier_id, contact_name, contact_role, phone, email, is_primary, created_at
             FROM inventory_supplier_contacts
             WHERE supplier_id = ?
             ORDER BY is_primary DESC, contact_name ASC",
            [supplier_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(SupplierContact {
                id: row.try_get("", "id")?,
                supplier_id: row.try_get("", "supplier_id")?,
                contact_name: row.try_get("", "contact_name")?,
                contact_role: row.try_get("", "contact_role")?,
                phone: row.try_get("", "phone")?,
                email: row.try_get("", "email")?,
                is_primary: row.try_get("", "is_primary")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn upsert_supplier_contact(
    db: &DatabaseConnection,
    contact_id: Option<i64>,
    input: SupplierContactInput,
) -> AppResult<SupplierContact> {
    let _ = get_supplier(db, input.supplier_id).await?;
    if input.contact_name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "contact_name is required.".to_string(),
        ]));
    }
    let is_primary = input.is_primary.unwrap_or(false) as i64;
    if is_primary == 1 {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_supplier_contacts SET is_primary = 0 WHERE supplier_id = ?",
            [input.supplier_id.into()],
        ))
        .await?;
    }

    let final_id = if let Some(id) = contact_id {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_supplier_contacts
             SET contact_name = ?, contact_role = ?, phone = ?, email = ?, is_primary = ?
             WHERE id = ? AND supplier_id = ?",
            [
                input.contact_name.trim().to_string().into(),
                input.contact_role.clone().map_or(Value::String(None), Value::from),
                input.phone.clone().map_or(Value::String(None), Value::from),
                input.email.clone().map_or(Value::String(None), Value::from),
                is_primary.into(),
                id.into(),
                input.supplier_id.into(),
            ],
        ))
        .await?;
        id
    } else {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_supplier_contacts
                (supplier_id, contact_name, contact_role, phone, email, is_primary, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                input.supplier_id.into(),
                input.contact_name.trim().to_string().into(),
                input.contact_role.clone().map_or(Value::String(None), Value::from),
                input.phone.clone().map_or(Value::String(None), Value::from),
                input.email.clone().map_or(Value::String(None), Value::from),
                is_primary.into(),
                now_iso().into(),
            ],
        ))
        .await?;
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "id").ok())
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["Unable to create supplier contact.".to_string()])
        })?
    };

    list_supplier_contacts(db, input.supplier_id)
        .await?
        .into_iter()
        .find(|c| c.id == final_id)
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_supplier_contacts".to_string(),
            id: final_id.to_string(),
        })
}

pub async fn delete_supplier_contact(db: &DatabaseConnection, contact_id: i64) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM inventory_supplier_contacts WHERE id = ?",
            [contact_id.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "inventory_supplier_contacts".to_string(),
            id: contact_id.to_string(),
        });
    }
    Ok(())
}

// ── Purchase history ─────────────────────────────────────────────────────────

pub async fn list_supplier_purchase_history(
    db: &DatabaseConnection,
    supplier_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<SupplierPurchaseHistoryRow>> {
    let _ = get_supplier(db, supplier_id).await?;
    let lim = limit.unwrap_or(10).clamp(1, 100);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.id AS purchase_order_id, po.po_number, po.ordered_at,
                    pol.article_id, a.article_code, a.article_name,
                    pol.ordered_qty, pol.unit_price, pol.status
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             JOIN articles a ON a.id = pol.article_id
             WHERE po.supplier_id = ?
             ORDER BY po.ordered_at DESC NULLS LAST, po.id DESC
             LIMIT ?",
            [supplier_id.into(), lim.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(SupplierPurchaseHistoryRow {
                purchase_order_id: row.try_get("", "purchase_order_id")?,
                po_number: row.try_get("", "po_number")?,
                ordered_at: row.try_get("", "ordered_at").ok().flatten(),
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                ordered_qty: row.try_get("", "ordered_qty")?,
                unit_price: row.try_get("", "unit_price").ok().flatten(),
                status: row.try_get("", "status")?,
            })
        })
        .collect()
}

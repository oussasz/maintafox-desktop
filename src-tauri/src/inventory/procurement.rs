use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    CreateProcurementRequisitionInput, CreatePurchaseOrderFromRequisitionInput, CreateRepairableOrderInput,
    GoodsReceipt, GoodsReceiptLine, InventoryStateEvent, ProcurementRequisition, ProcurementRequisitionLine,
    ProcurementSupplier, PurchaseOrder, PurchaseOrderDetail, PurchaseOrderLine, ReceiveGoodsInput,
    RepairVsReplaceResult, RepairableHistoryStats, RepairableOrder, RepairableOrderDetail,
    TransitionProcurementRequisitionInput, TransitionPurchaseOrderInput, TransitionRepairableOrderInput,
    UpdatePostingStateInput,
};

const PROC_STATUS_DOMAIN: &str = "inventory.procurement_status";
const ERP_POSTING_DOMAIN: &str = "inventory.erp_posting_state";
const REPAIRABLE_STATUS_DOMAIN: &str = "inventory.repairable_status";

/// `inventory_document_links.entity_type` discriminators (uppercase, as used by SUPPLIER / ARTICLE).
const PO_DOCUMENT_ENTITY_TYPE: &str = "PURCHASE_ORDER";
const REPAIRABLE_DOCUMENT_ENTITY_TYPE: &str = "REPAIRABLE_ORDER";

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn next_doc_number(prefix: &str) -> String {
    format!("{prefix}-{}", Utc::now().timestamp_millis())
}

/// Treat whitespace-only user input as absent so the column stays NULL instead of blank.
fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

async fn ensure_lookup_code_active<C: ConnectionTrait>(db: &C, domain_key: &str, code: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"
            SELECT lv.id
            FROM lookup_values lv
            JOIN lookup_domains ld ON ld.id = lv.domain_id
            WHERE ld.domain_key = ? AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL
            "#,
            [domain_key.into(), code.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Code '{code}' is not active in governed domain '{domain_key}'."
        )]));
    }
    Ok(())
}

async fn ensure_article_active<C: ConnectionTrait>(db: &C, article_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "articles".to_string(),
            id: article_id.to_string(),
        });
    };
    let is_active: i64 = row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Stock mutation rejected: article is inactive.".to_string(),
        ]));
    }
    Ok(())
}

async fn ensure_location_active<C: ConnectionTrait>(db: &C, location_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sl.warehouse_id, sl.is_active AS location_active, w.is_active AS warehouse_active
             FROM stock_locations sl
             JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE sl.id = ?",
            [location_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "stock_locations".to_string(),
            id: location_id.to_string(),
        });
    };
    let location_active: i64 = row.try_get("", "location_active")?;
    let warehouse_active: i64 = row.try_get("", "warehouse_active")?;
    if location_active == 0 || warehouse_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Stock mutation rejected: location or warehouse is inactive.".to_string(),
        ]));
    }
    Ok(row.try_get("", "warehouse_id")?)
}

async fn ensure_supplier_active<C: ConnectionTrait>(db: &C, supplier_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active, status_code FROM inventory_suppliers WHERE id = ?",
            [supplier_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "inventory_suppliers".to_string(),
            id: supplier_id.to_string(),
        });
    };
    let is_active: i64 = row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Selected supplier is inactive.".to_string()
        ]));
    }
    let status_code: String = row.try_get("", "status_code")?;
    if status_code == "BLOCKED" {
        return Err(AppError::ValidationFailed(vec![
            "Selected supplier is BLOCKED and cannot be used on purchase orders.".to_string(),
        ]));
    }
    Ok(())
}

pub(crate) async fn record_state_event<C: ConnectionTrait>(
    db: &C,
    entity_type: &str,
    entity_id: i64,
    from_status: Option<&str>,
    to_status: &str,
    actor_id: Option<i64>,
    reason: Option<&str>,
    note: Option<&str>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_state_events
            (entity_type, entity_id, from_status, to_status, actor_id, reason, note, changed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            entity_type.into(),
            entity_id.into(),
            from_status.map_or(Value::Int(None), Value::from),
            to_status.into(),
            actor_id.map_or(Value::BigInt(None), Value::from),
            reason.map_or(Value::String(None), Value::from),
            note.map_or(Value::String(None), Value::from),
            now_iso().into(),
        ],
    ))
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct BalanceSnapshot {
    on_hand: f64,
    reserved: f64,
}

async fn get_balance_snapshot<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    _warehouse_id: i64,
    location_id: i64,
) -> AppResult<BalanceSnapshot> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT on_hand_qty, reserved_qty
             FROM stock_balances
             WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    if let Some(row) = row {
        Ok(BalanceSnapshot {
            on_hand: row.try_get("", "on_hand_qty")?,
            reserved: row.try_get("", "reserved_qty")?,
        })
    } else {
        Ok(BalanceSnapshot {
            on_hand: 0.0,
            reserved: 0.0,
        })
    }
}

async fn upsert_balance<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    on_hand_qty: f64,
    reserved_qty: f64,
) -> AppResult<()> {
    let available_qty = on_hand_qty - reserved_qty;
    if on_hand_qty < 0.0 || reserved_qty < 0.0 || available_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Stock invariants violated (negative on-hand/reserved/available).".to_string(),
        ]));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_balances
            (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(article_id, location_id)
         DO UPDATE SET
            on_hand_qty = excluded.on_hand_qty,
            reserved_qty = excluded.reserved_qty,
            available_qty = excluded.available_qty,
            updated_at = excluded.updated_at",
        [
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            on_hand_qty.into(),
            reserved_qty.into(),
            available_qty.into(),
            now_iso().into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn append_stock_event<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    movement_type: &str,
    quantity: f64,
    source_type: &str,
    source_id: Option<i64>,
    source_ref: Option<&str>,
    reason: Option<&str>,
    performed_by_id: Option<i64>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions
            (article_id, warehouse_id, location_id, reservation_id, movement_type, quantity, source_type, source_id,
             source_ref, reason, performed_by_id, performed_at)
         VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            movement_type.into(),
            quantity.into(),
            source_type.into(),
            source_id.map_or(Value::BigInt(None), Value::from),
            source_ref.map_or(Value::String(None), Value::from),
            reason.map_or(Value::String(None), Value::from),
            performed_by_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
        ],
    ))
    .await?;
    Ok(())
}

fn guard_transition(current: &str, next: &str, allowed: &[(&str, &[&str])], entity: &str) -> AppResult<()> {
    let Some((_, allowed_next)) = allowed.iter().find(|(state, _)| *state == current) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "{entity} is in unsupported state '{current}'."
        )]));
    };
    if allowed_next.iter().any(|candidate| *candidate == next) {
        return Ok(());
    }
    Err(AppError::ValidationFailed(vec![format!(
        "Invalid lifecycle transition for {entity}: {current} -> {next}."
    )]))
}

pub async fn list_procurement_suppliers(db: &DatabaseConnection) -> AppResult<Vec<ProcurementSupplier>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code AS company_code, name AS company_name, is_active
             FROM inventory_suppliers
             WHERE is_active = 1
             ORDER BY name ASC"
                .to_string(),
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ProcurementSupplier {
                id: row.try_get("", "id")?,
                company_code: row.try_get("", "company_code")?,
                company_name: row.try_get("", "company_name")?,
                is_active: row.try_get("", "is_active")?,
            })
        })
        .collect()
}

pub async fn create_procurement_requisition(
    db: &DatabaseConnection,
    input: CreateProcurementRequisitionInput,
) -> AppResult<ProcurementRequisition> {
    if input.requested_qty <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "requested_qty must be greater than zero.".to_string(),
        ]));
    }
    ensure_lookup_code_active(db, PROC_STATUS_DOMAIN, "DRAFT").await?;
    ensure_lookup_code_active(db, ERP_POSTING_DOMAIN, "PENDING_POSTING").await?;
    ensure_article_active(db, input.article_id).await?;
    let preferred_location_id = input.preferred_location_id.unwrap_or(0);
    if preferred_location_id > 0 {
        ensure_location_active(db, preferred_location_id).await?;
    }
    if let Some(reservation_id) = input.source_reservation_id {
        let exists = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM stock_reservations WHERE id = ?",
                [reservation_id.into()],
            ))
            .await?;
        if exists.is_none() {
            return Err(AppError::ValidationFailed(vec![
                "source_reservation_id does not exist.".to_string(),
            ]));
        }
    }

    let tx = db.begin().await?;
    let req_number = next_doc_number("REQ");
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO procurement_requisitions
            (req_number, demand_source_type, demand_source_id, demand_source_ref, purchase_priority,
             status, posting_state, requested_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'DRAFT', 'PENDING_POSTING', ?, ?, ?)",
        [
            req_number.clone().into(),
            input.demand_source_type.into(),
            input.demand_source_id.map_or(Value::BigInt(None), Value::from),
            input.demand_source_ref.clone().map_or(Value::String(None), Value::from),
            input
                .purchase_priority
                .clone()
                .unwrap_or_else(|| "NORMAL".to_string())
                .into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            now_iso().into(),
        ],
    ))
    .await?;

    let req_id: i64 = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to create requisition.".to_string()]))?
        .try_get("", "id")?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO procurement_requisition_lines
            (requisition_id, article_id, preferred_location_id, requested_qty, demand_source_line_id,
             source_reservation_id, source_reorder_trigger, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'OPEN', ?)",
        [
            req_id.into(),
            input.article_id.into(),
            input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
            input.requested_qty.into(),
            input.demand_source_line_id.map_or(Value::BigInt(None), Value::from),
            input.source_reservation_id.map_or(Value::BigInt(None), Value::from),
            input
                .source_reorder_trigger
                .clone()
                .map_or(Value::String(None), Value::from),
            now_iso().into(),
        ],
    ))
    .await?;

    record_state_event(
        &tx,
        "requisition",
        req_id,
        None,
        "DRAFT",
        input.actor_id,
        input.reason.as_deref(),
        Some("requisition_created"),
    )
    .await?;
    tx.commit().await?;
    get_procurement_requisition(db, req_id).await
}

pub async fn transition_procurement_requisition(
    db: &DatabaseConnection,
    input: TransitionProcurementRequisitionInput,
) -> AppResult<ProcurementRequisition> {
    ensure_lookup_code_active(db, PROC_STATUS_DOMAIN, &input.next_status).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status, row_version FROM procurement_requisitions WHERE id = ?",
            [input.requisition_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "procurement_requisitions".to_string(),
            id: input.requisition_id.to_string(),
        });
    };
    let current: String = row.try_get("", "status")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    if row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Requisition row_version mismatch.".to_string()
        ]));
    }
    guard_transition(
        &current,
        &input.next_status,
        &[
            ("DRAFT", &["SUBMITTED", "CANCELLED"]),
            ("SUBMITTED", &["APPROVED", "REJECTED", "CANCELLED"]),
            ("APPROVED", &["CLOSED", "CANCELLED"]),
            ("PARTIALLY_RECEIVED", &["CLOSED", "CANCELLED"]),
            ("CLOSED", &[]),
            ("CANCELLED", &[]),
            ("REJECTED", &[]),
        ],
        "requisition",
    )?;

    // Rejection is a first-class approval outcome; reason is mandatory so audit/KPIs
    // can distinguish it from requester/system CANCELLED.
    if input.next_status == "REJECTED" {
        let reason = input.reason.as_deref().map(str::trim).unwrap_or("");
        if reason.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Rejection reason is required.".to_string()
            ]));
        }
    }

    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE procurement_requisitions
         SET status = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.next_status.clone().into(),
            now_iso().into(),
            input.requisition_id.into(),
        ],
    ))
    .await?;
    record_state_event(
        &tx,
        "requisition",
        input.requisition_id,
        Some(&current),
        &input.next_status,
        input.actor_id,
        input.reason.as_deref(),
        input.note.as_deref(),
    )
    .await?;
    tx.commit().await?;
    get_procurement_requisition(db, input.requisition_id).await
}

pub async fn create_purchase_order_from_requisition(
    db: &DatabaseConnection,
    input: CreatePurchaseOrderFromRequisitionInput,
) -> AppResult<PurchaseOrder> {
    // Resolve effective supplier_id: prefer new supplier_id, fallback to resolving from supplier_company_id
    let resolved_supplier_id: Option<i64> = if let Some(sid) = input.supplier_id {
        ensure_supplier_active(db, sid).await?;
        Some(sid)
    } else if let Some(company_id) = input.supplier_company_id {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM inventory_suppliers WHERE external_company_id = ? AND is_active = 1 LIMIT 1",
                [company_id.into()],
            ))
            .await?;
        if let Some(row) = row {
            let sid: i64 = row.try_get("", "id")?;
            ensure_supplier_active(db, sid).await?;
            Some(sid)
        } else {
            None
        }
    } else {
        None
    };

    // Keep legacy company_id for backward compat
    let legacy_company_id: Option<i64> = if input.supplier_company_id.is_some() {
        input.supplier_company_id
    } else if let Some(sid) = resolved_supplier_id {
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT external_company_id FROM inventory_suppliers WHERE id = ?",
            [sid.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "external_company_id").ok().flatten())
    } else {
        None
    };

    let req = get_procurement_requisition(db, input.requisition_id).await?;
    if req.status != "APPROVED" {
        return Err(AppError::ValidationFailed(vec![
            "Requisition must be APPROVED before creating a PO.".to_string(),
        ]));
    }

    let tx = db.begin().await?;
    let po_number = next_doc_number("PO");
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO purchase_orders
            (po_number, requisition_id, supplier_id, supplier_company_id, status, posting_state,
             ordered_by_id, ordered_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'DRAFT', 'PENDING_POSTING', ?, ?, ?, ?)",
        [
            po_number.clone().into(),
            input.requisition_id.into(),
            resolved_supplier_id.map_or(Value::BigInt(None), Value::from),
            legacy_company_id.map_or(Value::BigInt(None), Value::from),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            now_iso().into(),
            now_iso().into(),
        ],
    ))
    .await?;
    let po_id: i64 = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to create purchase order.".to_string()]))?
        .try_get("", "id")?;

    // Insert PO lines — auto-fill unit_price from effective price catalog
    let req_lines = tx
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rl.id, rl.article_id, rl.requested_qty, rl.source_reservation_id, rl.demand_source_line_id,
                    r.demand_source_type, r.demand_source_id, r.demand_source_ref
             FROM procurement_requisition_lines rl
             JOIN procurement_requisitions r ON r.id = rl.requisition_id
             WHERE rl.requisition_id = ?",
            [input.requisition_id.into()],
        ))
        .await?;

    for line in req_lines {
        let rl_id: i64 = line.try_get("", "id")?;
        let article_id: i64 = line.try_get("", "article_id")?;
        let requested_qty: f64 = line.try_get("", "requested_qty")?;
        let source_reservation_id: Option<i64> = line.try_get("", "source_reservation_id").ok().flatten();
        let demand_source_line_id: Option<i64> = line.try_get("", "demand_source_line_id").ok().flatten();
        let demand_source_type: String = line.try_get("", "demand_source_type")?;
        let demand_source_id: Option<i64> = line.try_get("", "demand_source_id").ok().flatten();
        let demand_source_ref: Option<String> = line.try_get("", "demand_source_ref").ok().flatten();

        // Resolve effective price
        let unit_price: Option<f64> = if let Some(sid) = resolved_supplier_id {
            let now = now_iso();
            let price_row = tx
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT unit_price FROM inventory_supplier_prices
                     WHERE supplier_id = ? AND article_id = ? AND is_active = 1
                       AND valid_from <= ? AND (valid_to IS NULL OR valid_to >= ?)
                     ORDER BY valid_from DESC LIMIT 1",
                    [sid.into(), article_id.into(), now.clone().into(), now.into()],
                ))
                .await?;
            if let Some(r) = price_row {
                r.try_get("", "unit_price").ok()
            } else {
                let hint_row = tx
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT unit_price_hint FROM inventory_supplier_article_sources
                         WHERE supplier_id = ? AND article_id = ? AND is_active = 1 AND unit_price_hint IS NOT NULL
                         ORDER BY is_preferred DESC, priority ASC LIMIT 1",
                        [sid.into(), article_id.into()],
                    ))
                    .await?;
                hint_row.and_then(|r| r.try_get("", "unit_price_hint").ok()).flatten()
            }
        } else {
            None
        };

        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO purchase_order_lines
             (purchase_order_id, requisition_line_id, article_id, ordered_qty, received_qty, unit_price,
              demand_source_type, demand_source_id, demand_source_ref, demand_source_line_id,
              source_reservation_id, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?, ?, 'OPEN', ?, ?)",
            [
                po_id.into(),
                rl_id.into(),
                article_id.into(),
                requested_qty.into(),
                unit_price.map_or(Value::Double(None), Value::from),
                demand_source_type.into(),
                demand_source_id.map_or(Value::BigInt(None), Value::from),
                demand_source_ref.map_or(Value::String(None), Value::from),
                demand_source_line_id.map_or(Value::BigInt(None), Value::from),
                source_reservation_id.map_or(Value::BigInt(None), Value::from),
                now_iso().into(),
                now_iso().into(),
            ],
        ))
        .await?;
    }

    record_state_event(
        &tx,
        "purchase_order",
        po_id,
        None,
        "DRAFT",
        input.actor_id,
        None,
        Some("po_created_from_requisition"),
    )
    .await?;
    tx.commit().await?;
    get_purchase_order(db, po_id).await
}

/// Fallback promise horizon (days) used when neither the supplier article source
/// nor the supplier master declares a lead time. Intentionally conservative so an
/// approved PO always carries a measurable delivery expectation.
const DEFAULT_PO_LEAD_TIME_DAYS: i64 = 7;

/// now + COALESCE(max lead time across the PO line article sources,
/// supplier default lead time, `DEFAULT_PO_LEAD_TIME_DAYS`).
async fn resolve_expected_delivery_date<C: ConnectionTrait>(db: &C, purchase_order_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT (SELECT MAX(sas.lead_time_days)
                     FROM purchase_order_lines pol
                     JOIN inventory_supplier_article_sources sas
                       ON sas.article_id = pol.article_id
                      AND sas.supplier_id = po.supplier_id
                      AND sas.is_active = 1
                     WHERE pol.purchase_order_id = po.id) AS source_lead_time_days,
                    s.default_lead_time_days
             FROM purchase_orders po
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             WHERE po.id = ?",
            [purchase_order_id.into()],
        ))
        .await?;
    let lead_time_days = row
        .and_then(|row| {
            row.try_get::<Option<i64>>("", "source_lead_time_days")
                .ok()
                .flatten()
                .or_else(|| row.try_get::<Option<i64>>("", "default_lead_time_days").ok().flatten())
        })
        .filter(|days| *days >= 0)
        .unwrap_or(DEFAULT_PO_LEAD_TIME_DAYS);
    Ok((Utc::now() + chrono::Duration::days(lead_time_days))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string())
}

pub async fn transition_purchase_order(
    db: &DatabaseConnection,
    input: TransitionPurchaseOrderInput,
) -> AppResult<PurchaseOrder> {
    ensure_lookup_code_active(db, PROC_STATUS_DOMAIN, &input.next_status).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status, row_version FROM purchase_orders WHERE id = ?",
            [input.purchase_order_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "purchase_orders".to_string(),
            id: input.purchase_order_id.to_string(),
        });
    };
    let current: String = row.try_get("", "status")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    if row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Purchase order row_version mismatch.".to_string()
        ]));
    }
    guard_transition(
        &current,
        &input.next_status,
        &[
            ("DRAFT", &["SUBMITTED", "CANCELLED"]),
            ("SUBMITTED", &["APPROVED", "CANCELLED"]),
            ("APPROVED", &["PARTIALLY_RECEIVED", "RECEIVED_CLOSED", "CANCELLED"]),
            ("PARTIALLY_RECEIVED", &["RECEIVED_CLOSED", "CANCELLED"]),
            ("RECEIVED_CLOSED", &[]),
            ("CANCELLED", &[]),
        ],
        "purchase order",
    )?;
    // Approval is the point where the delivery promise becomes measurable (KPI-019 Late PO %).
    // Never overwrite a date the buyer already agreed with the supplier.
    let expected_delivery_date = if input.next_status == "APPROVED" {
        Some(resolve_expected_delivery_date(db, input.purchase_order_id).await?)
    } else {
        None
    };

    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE purchase_orders
         SET status = ?, row_version = row_version + 1, updated_at = ?,
             approved_at = CASE WHEN ? = 'APPROVED' THEN ? ELSE approved_at END,
             approved_by_id = CASE WHEN ? = 'APPROVED' THEN ? ELSE approved_by_id END,
             expected_delivery_date = COALESCE(expected_delivery_date, ?)
         WHERE id = ?",
        [
            input.next_status.clone().into(),
            now_iso().into(),
            input.next_status.clone().into(),
            now_iso().into(),
            input.next_status.clone().into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            expected_delivery_date.map_or(Value::String(None), Value::from),
            input.purchase_order_id.into(),
        ],
    ))
    .await?;
    record_state_event(
        &tx,
        "purchase_order",
        input.purchase_order_id,
        Some(&current),
        &input.next_status,
        input.actor_id,
        input.reason.as_deref(),
        input.note.as_deref(),
    )
    .await?;
    tx.commit().await?;
    get_purchase_order(db, input.purchase_order_id).await
}

pub async fn receive_purchase_order_goods(
    db: &DatabaseConnection,
    input: ReceiveGoodsInput,
) -> AppResult<GoodsReceipt> {
    if input.lines.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "At least one receipt line is required.".to_string(),
        ]));
    }
    let po = get_purchase_order(db, input.purchase_order_id).await?;
    if po.status != "APPROVED" && po.status != "PARTIALLY_RECEIVED" {
        return Err(AppError::ValidationFailed(vec![
            "PO must be APPROVED or PARTIALLY_RECEIVED before receiving goods.".to_string(),
        ]));
    }

    let tx = db.begin().await?;
    let gr_number = next_doc_number("GR");
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO goods_receipts
            (gr_number, purchase_order_id, status, posting_state, received_by_id, received_at, created_at, updated_at)
         VALUES (?, ?, 'RECEIVED', 'PENDING_POSTING', ?, ?, ?, ?)",
        [
            gr_number.clone().into(),
            input.purchase_order_id.into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            now_iso().into(),
            now_iso().into(),
        ],
    ))
    .await?;
    let gr_id: i64 = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to create goods receipt.".to_string()]))?
        .try_get("", "id")?;

    for line in &input.lines {
        if line.received_qty <= 0.0 || line.accepted_qty < 0.0 || line.rejected_qty < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "received_qty must be > 0 and accepted/rejected must be >= 0.".to_string(),
            ]));
        }
        let sum = line.accepted_qty + line.rejected_qty;
        if (sum - line.received_qty).abs() > f64::EPSILON {
            return Err(AppError::ValidationFailed(vec![
                "accepted_qty + rejected_qty must equal received_qty.".to_string(),
            ]));
        }
        ensure_article_active(&tx, line.article_id).await?;
        let warehouse_id = ensure_location_active(&tx, line.location_id).await?;

        let po_line_row = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ordered_qty, received_qty, demand_source_type, demand_source_id, demand_source_ref
                 FROM purchase_order_lines
                 WHERE id = ? AND purchase_order_id = ?",
                [line.po_line_id.into(), input.purchase_order_id.into()],
            ))
            .await?;
        let Some(po_line_row) = po_line_row else {
            return Err(AppError::ValidationFailed(vec![format!(
                "PO line {} does not belong to purchase order {}.",
                line.po_line_id, input.purchase_order_id
            )]));
        };
        let ordered_qty: f64 = po_line_row.try_get("", "ordered_qty")?;
        let current_received_qty: f64 = po_line_row.try_get("", "received_qty")?;
        if current_received_qty + line.received_qty > ordered_qty + f64::EPSILON {
            return Err(AppError::ValidationFailed(vec![
                "Receiving quantity exceeds ordered quantity.".to_string(),
            ]));
        }

        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO goods_receipt_lines
             (goods_receipt_id, po_line_id, article_id, location_id, received_qty, accepted_qty, rejected_qty,
              rejection_reason, ordered_qty, actual_lead_time_days, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'RECEIVED', ?)",
            [
                gr_id.into(),
                line.po_line_id.into(),
                line.article_id.into(),
                line.location_id.into(),
                line.received_qty.into(),
                line.accepted_qty.into(),
                line.rejected_qty.into(),
                line.rejection_reason.clone().map_or(Value::String(None), Value::from),
                ordered_qty.into(),
                {
                    // Compute actual lead time from PO ordered_at/approved_at to now
                    let reference_date = po.ordered_at.as_deref().or(po.approved_at.as_deref());
                    if let Some(ref_date) = reference_date {
                        if let (Ok(start), Ok(end)) = (
                            chrono::DateTime::parse_from_rfc3339(ref_date),
                            chrono::DateTime::parse_from_rfc3339(&now_iso()),
                        ) {
                            let days = (end - start).num_seconds() as f64 / 86400.0;
                            Value::Double(Some(days.into()))
                        } else {
                            Value::Double(None)
                        }
                    } else {
                        Value::Double(None)
                    }
                },
                now_iso().into(),
            ],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE purchase_order_lines
             SET received_qty = received_qty + ?, status = CASE WHEN received_qty + ? >= ordered_qty THEN 'CLOSED' ELSE 'OPEN' END,
                 updated_at = ?
             WHERE id = ?",
            [
                line.received_qty.into(),
                line.received_qty.into(),
                now_iso().into(),
                line.po_line_id.into(),
            ],
        ))
        .await?;

        if line.accepted_qty > 0.0 {
            let before = get_balance_snapshot(&tx, line.article_id, warehouse_id, line.location_id).await?;
            upsert_balance(
                &tx,
                line.article_id,
                warehouse_id,
                line.location_id,
                before.on_hand + line.accepted_qty,
                before.reserved,
            )
            .await?;
            append_stock_event(
                &tx,
                line.article_id,
                warehouse_id,
                line.location_id,
                "GR_ACCEPT",
                line.accepted_qty,
                "PO_GR",
                Some(gr_id),
                Some(&gr_number),
                line.rejection_reason.as_deref(),
                input.actor_id,
            )
            .await?;
        }
    }

    let open_line_count: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM purchase_order_lines WHERE purchase_order_id = ? AND status = 'OPEN'",
            [input.purchase_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to evaluate PO status.".to_string()]))?
        .try_get("", "cnt")?;

    let next_po_status = if open_line_count == 0 {
        "RECEIVED_CLOSED"
    } else {
        "PARTIALLY_RECEIVED"
    };
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE purchase_orders SET status = ?, updated_at = ?, row_version = row_version + 1 WHERE id = ?",
        [next_po_status.into(), now_iso().into(), input.purchase_order_id.into()],
    ))
    .await?;
    record_state_event(
        &tx,
        "purchase_order",
        input.purchase_order_id,
        Some(&po.status),
        next_po_status,
        input.actor_id,
        None,
        Some("goods_received"),
    )
    .await?;

    tx.commit().await?;

    // Post-commit: handle fulfillment_action for WO-linked demand
    if let Some(ref action) = input.fulfillment_action {
        if matches!(action.as_str(), "RESERVE_FOR_DEMAND" | "ISSUE_TO_DEMAND") {
            // For each accepted line, check if there's an open reservation linked to the PO demand
            for line in &input.lines {
                if line.accepted_qty <= 0.0 {
                    continue;
                }
                // Find open reservation for this article/demand combo
                let res_row = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT sr.id, sr.quantity_reserved, sr.quantity_issued
                         FROM stock_reservations sr
                         JOIN purchase_order_lines pol ON pol.source_reservation_id = sr.id
                         WHERE pol.id = ? AND sr.status = 'ACTIVE'
                         LIMIT 1",
                        [line.po_line_id.into()],
                    ))
                    .await
                    .ok()
                    .flatten();

                if let Some(res_row) = res_row {
                    let reservation_id: i64 = res_row.try_get("", "id").unwrap_or(0);
                    if reservation_id > 0 && action == "ISSUE_TO_DEMAND" {
                        let _ = crate::inventory::queries::issue_reserved_stock(
                            db,
                            crate::inventory::domain::InventoryIssueInput {
                                reservation_id,
                                quantity: line.accepted_qty,
                                source_type: Some("PO_GR".to_string()),
                                source_id: Some(line.po_line_id),
                                source_ref: None,
                                notes: Some(format!("Auto-issued on GR accept (fulfillment_action=ISSUE_TO_DEMAND)")),
                            },
                        )
                        .await
                        .ok(); // best-effort: do not fail GR on issue error
                    }
                }
            }
        }
    }

    get_goods_receipt(db, gr_id).await
}

pub async fn update_procurement_posting_state(
    db: &DatabaseConnection,
    input: UpdatePostingStateInput,
) -> AppResult<()> {
    ensure_lookup_code_active(db, ERP_POSTING_DOMAIN, &input.posting_state).await?;
    let table = match input.entity_type.as_str() {
        "requisition" => "procurement_requisitions",
        "purchase_order" => "purchase_orders",
        "goods_receipt" => "goods_receipts",
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "Unsupported posting entity_type '{other}'."
            )]))
        }
    };
    let sql = format!("UPDATE {table} SET posting_state = ?, posting_error = ?, updated_at = ? WHERE id = ?");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [
            input.posting_state.into(),
            input.posting_error.map_or(Value::String(None), Value::from),
            now_iso().into(),
            input.entity_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn create_repairable_order(
    db: &DatabaseConnection,
    input: CreateRepairableOrderInput,
) -> AppResult<RepairableOrder> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Repairable quantity must be greater than zero.".to_string(),
        ]));
    }
    ensure_lookup_code_active(db, REPAIRABLE_STATUS_DOMAIN, "REQUESTED").await?;
    ensure_article_active(db, input.article_id).await?;
    ensure_location_active(db, input.source_location_id).await?;
    if let Some(return_location_id) = input.return_location_id {
        ensure_location_active(db, return_location_id).await?;
    }
    if let Some(vendor_supplier_id) = input.vendor_supplier_id {
        ensure_supplier_active(db, vendor_supplier_id).await?;
    }
    let code = next_doc_number("REP");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO repairable_orders
            (order_code, article_id, quantity, source_location_id, return_location_id, linked_po_line_id, linked_reservation_id,
             status, reason, serial_number, vendor_supplier_id, created_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'REQUESTED', ?, ?, ?, ?, ?, ?)",
        [
            code.clone().into(),
            input.article_id.into(),
            input.quantity.into(),
            input.source_location_id.into(),
            input.return_location_id.map_or(Value::BigInt(None), Value::from),
            input.linked_po_line_id.map_or(Value::BigInt(None), Value::from),
            input.linked_reservation_id.map_or(Value::BigInt(None), Value::from),
            input.reason.clone().map_or(Value::String(None), Value::from),
            normalize_optional_text(input.serial_number.clone()).map_or(Value::String(None), Value::from),
            input.vendor_supplier_id.map_or(Value::BigInt(None), Value::from),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
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
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to create repairable order.".to_string()]))?
        .try_get("", "id")?;
    record_state_event(
        db,
        "repairable_order",
        id,
        None,
        "REQUESTED",
        input.actor_id,
        input.reason.as_deref(),
        Some("repairable_requested"),
    )
    .await?;
    get_repairable_order(db, id).await
}

pub async fn transition_repairable_order(
    db: &DatabaseConnection,
    input: TransitionRepairableOrderInput,
) -> AppResult<RepairableOrder> {
    ensure_lookup_code_active(db, REPAIRABLE_STATUS_DOMAIN, &input.next_status).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_id, quantity, source_location_id, return_location_id, status, row_version
             FROM repairable_orders
             WHERE id = ?",
            [input.order_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "repairable_orders".to_string(),
            id: input.order_id.to_string(),
        });
    };
    let current_status: String = row.try_get("", "status")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    if row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Repairable order row_version mismatch.".to_string(),
        ]));
    }
    guard_transition(
        &current_status,
        &input.next_status,
        &[
            ("REQUESTED", &["RELEASED", "CANCELLED"]),
            ("RELEASED", &["SENT_FOR_REPAIR", "CANCELLED"]),
            ("SENT_FOR_REPAIR", &["RETURNED_FROM_REPAIR", "SCRAPPED"]),
            ("RETURNED_FROM_REPAIR", &["CLOSED", "SCRAPPED"]),
            ("CLOSED", &[]),
            ("SCRAPPED", &[]),
            ("CANCELLED", &[]),
        ],
        "repairable order",
    )?;

    if input.repair_cost.is_some_and(|cost| cost < 0.0 || !cost.is_finite()) {
        return Err(AppError::ValidationFailed(vec![
            "repair_cost must be a positive amount.".to_string(),
        ]));
    }
    if let Some(vendor_supplier_id) = input.vendor_supplier_id {
        ensure_supplier_active(db, vendor_supplier_id).await?;
    }

    let article_id: i64 = row.try_get("", "article_id")?;
    let quantity: f64 = row.try_get("", "quantity")?;
    let source_location_id: i64 = row.try_get("", "source_location_id")?;
    let current_return_location_id: Option<i64> = row.try_get("", "return_location_id")?;

    let tx = db.begin().await?;
    if input.next_status == "RELEASED" {
        let warehouse_id = ensure_location_active(&tx, source_location_id).await?;
        let before = get_balance_snapshot(&tx, article_id, warehouse_id, source_location_id).await?;
        if before.on_hand - before.reserved < quantity {
            return Err(AppError::ValidationFailed(vec![
                "Insufficient available stock to release repairable.".to_string(),
            ]));
        }
        upsert_balance(
            &tx,
            article_id,
            warehouse_id,
            source_location_id,
            before.on_hand - quantity,
            before.reserved,
        )
        .await?;
        append_stock_event(
            &tx,
            article_id,
            warehouse_id,
            source_location_id,
            "REPAIRABLE_RELEASE",
            quantity,
            "REPAIRABLE_ORDER",
            Some(input.order_id),
            None,
            input.reason.as_deref(),
            input.actor_id,
        )
        .await?;
    }
    if input.next_status == "RETURNED_FROM_REPAIR" {
        let target_location_id = input
            .return_location_id
            .or(current_return_location_id)
            .ok_or_else(|| AppError::ValidationFailed(vec!["return_location_id is required on return.".to_string()]))?;
        let warehouse_id = ensure_location_active(&tx, target_location_id).await?;
        let before = get_balance_snapshot(&tx, article_id, warehouse_id, target_location_id).await?;
        upsert_balance(
            &tx,
            article_id,
            warehouse_id,
            target_location_id,
            before.on_hand + quantity,
            before.reserved,
        )
        .await?;
        append_stock_event(
            &tx,
            article_id,
            warehouse_id,
            target_location_id,
            "REPAIRABLE_RETURN",
            quantity,
            "REPAIRABLE_ORDER",
            Some(input.order_id),
            None,
            input.reason.as_deref(),
            input.actor_id,
        )
        .await?;
    }

    // Cancel / scrap stock legs branch on *current* status: the same next_status has
    // opposite inventory meaning depending on how far the part has moved.
    // - CANCELLED from REQUESTED: nothing was issued → no leg
    // - CANCELLED from RELEASED: restore quantity to source
    // - SCRAPPED from SENT_FOR_REPAIR: already written off at release → no leg
    // - SCRAPPED from RETURNED_FROM_REPAIR: write off from return location
    if input.next_status == "CANCELLED" && current_status == "RELEASED" {
        let warehouse_id = ensure_location_active(&tx, source_location_id).await?;
        let before = get_balance_snapshot(&tx, article_id, warehouse_id, source_location_id).await?;
        upsert_balance(
            &tx,
            article_id,
            warehouse_id,
            source_location_id,
            before.on_hand + quantity,
            before.reserved,
        )
        .await?;
        append_stock_event(
            &tx,
            article_id,
            warehouse_id,
            source_location_id,
            "REPAIRABLE_CANCEL_RESTORE",
            quantity,
            "REPAIRABLE_ORDER",
            Some(input.order_id),
            None,
            input.reason.as_deref(),
            input.actor_id,
        )
        .await?;
    }
    if input.next_status == "SCRAPPED" && current_status == "RETURNED_FROM_REPAIR" {
        let target_location_id = input.return_location_id.or(current_return_location_id).ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "return_location_id is required to scrap a returned repairable.".to_string()
            ])
        })?;
        let warehouse_id = ensure_location_active(&tx, target_location_id).await?;
        let before = get_balance_snapshot(&tx, article_id, warehouse_id, target_location_id).await?;
        if before.on_hand < quantity {
            return Err(AppError::ValidationFailed(vec![
                "Insufficient on-hand stock at return location to scrap repairable.".to_string(),
            ]));
        }
        upsert_balance(
            &tx,
            article_id,
            warehouse_id,
            target_location_id,
            before.on_hand - quantity,
            before.reserved,
        )
        .await?;
        append_stock_event(
            &tx,
            article_id,
            warehouse_id,
            target_location_id,
            "REPAIRABLE_SCRAP_WRITEOFF",
            quantity,
            "REPAIRABLE_ORDER",
            Some(input.order_id),
            None,
            input.reason.as_deref(),
            input.actor_id,
        )
        .await?;
    }

    // Dispatch/return timestamps are stamped once by the lifecycle so turnaround stays auditable
    // (KPI-015). CLOSED after RETURNED_FROM_REPAIR must not overwrite the actual return date.
    let sent_at = (input.next_status == "SENT_FOR_REPAIR").then(now_iso);
    let returned_at = matches!(input.next_status.as_str(), "RETURNED_FROM_REPAIR" | "CLOSED").then(now_iso);

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE repairable_orders
         SET status = ?, return_location_id = COALESCE(?, return_location_id),
             serial_number = COALESCE(?, serial_number),
             vendor_supplier_id = COALESCE(?, vendor_supplier_id),
             repair_cost = COALESCE(?, repair_cost),
             warranty_active = COALESCE(?, warranty_active),
             warranty_until = COALESCE(?, warranty_until),
             sent_at = COALESCE(sent_at, ?),
             returned_at = COALESCE(returned_at, ?),
             row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.next_status.clone().into(),
            input.return_location_id.map_or(Value::BigInt(None), Value::from),
            normalize_optional_text(input.serial_number.clone()).map_or(Value::String(None), Value::from),
            input.vendor_supplier_id.map_or(Value::BigInt(None), Value::from),
            input.repair_cost.map_or(Value::Double(None), Value::from),
            input
                .warranty_active
                .map_or(Value::BigInt(None), |active| Value::from(i64::from(active))),
            input.warranty_until.clone().map_or(Value::String(None), Value::from),
            sent_at.map_or(Value::String(None), Value::from),
            returned_at.map_or(Value::String(None), Value::from),
            now_iso().into(),
            input.order_id.into(),
        ],
    ))
    .await?;
    record_state_event(
        &tx,
        "repairable_order",
        input.order_id,
        Some(&current_status),
        &input.next_status,
        input.actor_id,
        input.reason.as_deref(),
        input.note.as_deref(),
    )
    .await?;
    tx.commit().await?;
    get_repairable_order(db, input.order_id).await
}

pub async fn list_procurement_requisitions(db: &DatabaseConnection) -> AppResult<Vec<ProcurementRequisition>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, req_number, demand_source_type, demand_source_id, demand_source_ref,
                    COALESCE(purchase_priority, 'NORMAL') AS purchase_priority,
                    status, posting_state, posting_error, requested_by_id, row_version, created_at, updated_at
             FROM procurement_requisitions
             ORDER BY id DESC"
                .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_requisition_row).collect()
}

pub async fn list_procurement_requisition_lines(
    db: &DatabaseConnection,
    requisition_id: i64,
) -> AppResult<Vec<ProcurementRequisitionLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rl.id, rl.requisition_id, rl.article_id, a.article_code, a.article_name,
                    rl.preferred_location_id, sl.code AS preferred_location_code, rl.requested_qty,
                    rl.demand_source_line_id, rl.source_reservation_id, rl.source_reorder_trigger,
                    rl.status, rl.created_at
             FROM procurement_requisition_lines rl
             JOIN articles a ON a.id = rl.article_id
             LEFT JOIN stock_locations sl ON sl.id = rl.preferred_location_id
             WHERE rl.requisition_id = ?
             ORDER BY rl.id ASC",
            [requisition_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_requisition_line_row).collect()
}

pub async fn list_purchase_orders(db: &DatabaseConnection) -> AppResult<Vec<PurchaseOrder>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT po.id, po.po_number, po.requisition_id,
                    po.supplier_id, s.name AS supplier_name,
                    po.supplier_company_id, ec.name AS supplier_company_name,
                    po.status, po.posting_state, po.posting_error, po.ordered_by_id, po.ordered_at,
                    po.approved_by_id, po.approved_at, po.expected_delivery_date,
                    po.row_version, po.created_at, po.updated_at
             FROM purchase_orders po
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             LEFT JOIN external_companies ec ON ec.id = po.supplier_company_id
             ORDER BY po.id DESC"
                .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_purchase_order_row).collect()
}

pub async fn list_purchase_order_lines(
    db: &DatabaseConnection,
    purchase_order_id: i64,
) -> AppResult<Vec<PurchaseOrderLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pol.id, pol.purchase_order_id, pol.requisition_line_id, pol.article_id, a.article_code, a.article_name,
                    pol.ordered_qty, pol.received_qty, pol.unit_price, pol.demand_source_type, pol.demand_source_id,
                    pol.demand_source_ref, pol.demand_source_line_id, pol.source_reservation_id, pol.status,
                    CASE WHEN pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
                         THEN pol.demand_source_id END AS work_order_id,
                    CASE WHEN pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
                         THEN COALESCE(wo.code, pol.demand_source_ref) END AS work_order_code,
                    pol.created_at, pol.updated_at
             FROM purchase_order_lines pol
             JOIN articles a ON a.id = pol.article_id
             LEFT JOIN work_orders wo
                    ON wo.id = pol.demand_source_id
                   AND pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
             WHERE pol.purchase_order_id = ?
             ORDER BY pol.id ASC",
            [purchase_order_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_po_line_row).collect()
}

/// Single round-trip aggregate backing the purchase order detail workspace.
pub async fn get_purchase_order_detail(
    db: &DatabaseConnection,
    purchase_order_id: i64,
) -> AppResult<PurchaseOrderDetail> {
    let order = get_purchase_order(db, purchase_order_id).await?;
    let lines = list_purchase_order_lines(db, purchase_order_id).await?;

    let goods_receipts = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, gr_number, purchase_order_id, status, posting_state, posting_error, received_by_id, received_at,
                    row_version, created_at, updated_at
             FROM goods_receipts
             WHERE purchase_order_id = ?
             ORDER BY id DESC",
            [purchase_order_id.into()],
        ))
        .await?
        .into_iter()
        .map(map_goods_receipt_row)
        .collect::<AppResult<Vec<GoodsReceipt>>>()?;

    let state_events =
        list_inventory_state_events(db, Some("purchase_order".to_string()), Some(purchase_order_id)).await?;
    let document_links =
        crate::inventory::queries::list_inventory_document_links(db, PO_DOCUMENT_ENTITY_TYPE, purchase_order_id)
            .await?;

    let priced_total: f64 = lines.iter().filter_map(|line| line.line_total).sum();
    let priced_line_count = lines.iter().filter(|line| line.line_total.is_some()).count();
    let grand_total_partial = priced_line_count < lines.len();

    Ok(PurchaseOrderDetail {
        order,
        goods_receipts,
        state_events,
        document_links,
        grand_total: (priced_line_count > 0).then_some(priced_total),
        grand_total_partial,
        lines,
    })
}

pub async fn list_goods_receipts(db: &DatabaseConnection) -> AppResult<Vec<GoodsReceipt>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, gr_number, purchase_order_id, status, posting_state, posting_error, received_by_id, received_at,
                    row_version, created_at, updated_at
             FROM goods_receipts
             ORDER BY id DESC"
            .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_goods_receipt_row).collect()
}

pub async fn list_goods_receipt_lines(
    db: &DatabaseConnection,
    goods_receipt_id: i64,
) -> AppResult<Vec<GoodsReceiptLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT grl.id, grl.goods_receipt_id, grl.po_line_id, grl.article_id, a.article_code, a.article_name,
                    grl.location_id, sl.code AS location_code, grl.received_qty, grl.accepted_qty, grl.rejected_qty,
                    grl.rejection_reason, grl.ordered_qty, grl.actual_lead_time_days, grl.status, grl.created_at
             FROM goods_receipt_lines grl
             JOIN articles a ON a.id = grl.article_id
             JOIN stock_locations sl ON sl.id = grl.location_id
             WHERE grl.goods_receipt_id = ?
             ORDER BY grl.id ASC",
            [goods_receipt_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_goods_receipt_line_row).collect()
}

/// Shared projection for repairable orders: vendor label, WO business code (resolved through the
/// linked reservation) and repair facts. Callers append their own WHERE / ORDER BY.
const REPAIRABLE_ORDER_SELECT: &str =
    "SELECT ro.id, ro.order_code, ro.article_id, a.article_code, a.article_name, ro.quantity,
            ro.source_location_id, src.code AS source_location_code, ro.return_location_id,
            ret.code AS return_location_code, ro.linked_po_line_id, ro.linked_reservation_id, ro.status,
            ro.reason, ro.serial_number, ro.vendor_supplier_id,
            vs.code AS vendor_supplier_code, vs.name AS vendor_supplier_name,
            ro.sent_at, ro.returned_at, ro.warranty_active, ro.warranty_until, ro.repair_cost,
            CASE WHEN sr.source_type IN ('WORK_ORDER', 'WORK_ORDER_PART') THEN sr.source_id END AS work_order_id,
            CASE WHEN sr.source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
                 THEN COALESCE(wo.code, sr.source_ref) END AS work_order_code,
            ro.created_by_id, ro.row_version, ro.created_at, ro.updated_at
     FROM repairable_orders ro
     JOIN articles a ON a.id = ro.article_id
     JOIN stock_locations src ON src.id = ro.source_location_id
     LEFT JOIN stock_locations ret ON ret.id = ro.return_location_id
     LEFT JOIN inventory_suppliers vs ON vs.id = ro.vendor_supplier_id
     LEFT JOIN stock_reservations sr ON sr.id = ro.linked_reservation_id
     LEFT JOIN work_orders wo ON wo.id = sr.source_id AND sr.source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')";

pub async fn list_repairable_orders(db: &DatabaseConnection) -> AppResult<Vec<RepairableOrder>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{REPAIRABLE_ORDER_SELECT} ORDER BY ro.id DESC"),
        ))
        .await?;
    rows.into_iter().map(map_repairable_row).collect()
}

/// Tenant setting key for the repair/replace decision boundary.
const REPAIR_REPLACE_RATIO_SETTING_KEY: &str = "procurement.repair_replace_ratio";
/// Applied when the tenant has not configured `procurement.repair_replace_ratio`:
/// repair is recommended while it costs at most 85% of replacement.
const DEFAULT_REPAIR_REPLACE_RATIO: f64 = 0.85;
/// Ratio distance from the threshold within which the call is too close to automate.
const REPAIR_REPLACE_REVIEW_BAND: f64 = 0.05;

async fn resolve_repair_replace_ratio(db: &DatabaseConnection) -> AppResult<f64> {
    let configured = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT CAST(setting_value_json AS REAL) AS ratio FROM app_settings WHERE setting_key = ? LIMIT 1",
            [REPAIR_REPLACE_RATIO_SETTING_KEY.into()],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<f64>>("", "ratio").ok().flatten())
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    Ok(configured.unwrap_or(DEFAULT_REPAIR_REPLACE_RATIO))
}

async fn get_repairable_history_stats(db: &DatabaseConnection, article_id: i64) -> AppResult<RepairableHistoryStats> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS repair_count,
                    AVG(repair_cost) AS avg_cost,
                    AVG(CASE WHEN sent_at IS NOT NULL AND returned_at IS NOT NULL
                             THEN julianday(returned_at) - julianday(sent_at) END) AS avg_turnaround_days
             FROM repairable_orders
             WHERE article_id = ? AND status IN ('RETURNED_FROM_REPAIR', 'CLOSED')",
            [article_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(RepairableHistoryStats {
            repair_count: 0,
            avg_cost: None,
            avg_turnaround_days: None,
        });
    };
    Ok(RepairableHistoryStats {
        repair_count: row.try_get("", "repair_count").unwrap_or(0),
        avg_cost: row.try_get("", "avg_cost").ok().flatten(),
        avg_turnaround_days: row.try_get("", "avg_turnaround_days").ok().flatten(),
    })
}

/// Compares the repair cost of an order against the valuation cost of replacing the same quantity.
///
/// Repair cost falls back to the article's historical average when the order has none yet;
/// replacement cost comes from the active valuation policy (`valuation::evaluate_unit_cost`).
/// `reason` carries a stable machine code so the UI can translate it.
pub async fn evaluate_repair_vs_replace(db: &DatabaseConnection, order_id: i64) -> AppResult<RepairVsReplaceResult> {
    let order = get_repairable_order(db, order_id).await?;
    let history = get_repairable_history_stats(db, order.article_id).await?;
    let threshold_ratio = resolve_repair_replace_ratio(db).await?;

    let repair_cost_is_estimated = order.repair_cost.is_none();
    let repair_cost = order
        .repair_cost
        .or(history.avg_cost)
        .filter(|cost| cost.is_finite() && *cost > 0.0);

    let warehouse_id: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT warehouse_id FROM stock_locations WHERE id = ?",
            [order.source_location_id.into()],
        ))
        .await?
        .and_then(|row| row.try_get("", "warehouse_id").ok());

    let valuation = match warehouse_id {
        Some(warehouse_id) => crate::inventory::valuation::evaluate_unit_cost(
            db,
            order.article_id,
            warehouse_id,
            order.source_location_id,
        )
        .await
        .ok(),
        None => None,
    };
    let replacement_cost = valuation
        .as_ref()
        .map(|cost| cost.unit_cost * order.quantity)
        .filter(|cost| cost.is_finite() && *cost > 0.0);
    let replacement_is_provisional = valuation
        .as_ref()
        .map_or(false, |cost| cost.is_provisional || cost.confidence == "low");

    let (Some(repair_cost), Some(replacement_cost)) = (repair_cost, replacement_cost) else {
        let reason = if repair_cost.is_none() {
            "MISSING_REPAIR_COST"
        } else {
            "MISSING_REPLACEMENT_COST"
        };
        return Ok(RepairVsReplaceResult {
            repair_cost,
            replacement_cost,
            threshold_ratio,
            recommendation: "INSUFFICIENT_DATA".to_string(),
            reason: Some(reason.to_string()),
        });
    };

    let ratio = repair_cost / replacement_cost;
    let near_threshold = (ratio - threshold_ratio).abs() <= REPAIR_REPLACE_REVIEW_BAND;
    let (recommendation, reason) = if near_threshold {
        ("REVIEW", Some("RATIO_NEAR_THRESHOLD"))
    } else if repair_cost_is_estimated {
        ("REVIEW", Some("REPAIR_COST_FROM_HISTORY_AVG"))
    } else if replacement_is_provisional {
        ("REVIEW", Some("REPLACEMENT_COST_PROVISIONAL"))
    } else if ratio <= threshold_ratio {
        ("REPAIR", None)
    } else {
        ("REPLACE", None)
    };

    Ok(RepairVsReplaceResult {
        repair_cost: Some(repair_cost),
        replacement_cost: Some(replacement_cost),
        threshold_ratio,
        recommendation: recommendation.to_string(),
        reason: reason.map(str::to_string),
    })
}

/// Single round-trip aggregate backing the repairable order detail workspace.
pub async fn get_repairable_order_detail(db: &DatabaseConnection, order_id: i64) -> AppResult<RepairableOrderDetail> {
    let order = get_repairable_order(db, order_id).await?;
    let state_events = list_inventory_state_events(db, Some("repairable_order".to_string()), Some(order_id)).await?;
    let document_links =
        crate::inventory::queries::list_inventory_document_links(db, REPAIRABLE_DOCUMENT_ENTITY_TYPE, order_id).await?;
    let history_stats = get_repairable_history_stats(db, order.article_id).await?;
    let repair_vs_replace = evaluate_repair_vs_replace(db, order_id).await?;

    Ok(RepairableOrderDetail {
        order,
        state_events,
        document_links,
        history_stats,
        repair_vs_replace,
    })
}

pub async fn list_inventory_state_events(
    db: &DatabaseConnection,
    entity_type: Option<String>,
    entity_id: Option<i64>,
) -> AppResult<Vec<InventoryStateEvent>> {
    let mut sql = "SELECT id, entity_type, entity_id, from_status, to_status, actor_id, reason, note, changed_at
                   FROM inventory_state_events
                   WHERE 1=1"
        .to_string();
    let mut values: Vec<Value> = Vec::new();
    if let Some(entity_type) = entity_type {
        sql.push_str(" AND entity_type = ?");
        values.push(entity_type.into());
    }
    if let Some(entity_id) = entity_id {
        sql.push_str(" AND entity_id = ?");
        values.push(entity_id.into());
    }
    sql.push_str(" ORDER BY changed_at DESC, id DESC");
    let rows = if values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
    };
    rows.into_iter()
        .map(|row| {
            Ok(InventoryStateEvent {
                id: row.try_get("", "id")?,
                entity_type: row.try_get("", "entity_type")?,
                entity_id: row.try_get("", "entity_id")?,
                from_status: row.try_get("", "from_status")?,
                to_status: row.try_get("", "to_status")?,
                actor_id: row.try_get("", "actor_id")?,
                reason: row.try_get("", "reason")?,
                note: row.try_get("", "note")?,
                changed_at: row.try_get("", "changed_at")?,
            })
        })
        .collect()
}

async fn get_procurement_requisition(
    db: &DatabaseConnection,
    requisition_id: i64,
) -> AppResult<ProcurementRequisition> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, req_number, demand_source_type, demand_source_id, demand_source_ref,
                    COALESCE(purchase_priority, 'NORMAL') AS purchase_priority,
                    status, posting_state, posting_error, requested_by_id, row_version, created_at, updated_at
             FROM procurement_requisitions WHERE id = ?",
            [requisition_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "procurement_requisitions".to_string(),
            id: requisition_id.to_string(),
        })?;
    map_requisition_row(row)
}

async fn get_purchase_order(db: &DatabaseConnection, purchase_order_id: i64) -> AppResult<PurchaseOrder> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.id, po.po_number, po.requisition_id,
                    po.supplier_id, s.name AS supplier_name,
                    po.supplier_company_id, ec.name AS supplier_company_name,
                    po.status, po.posting_state, po.posting_error, po.ordered_by_id, po.ordered_at,
                    po.approved_by_id, po.approved_at, po.expected_delivery_date,
                    po.row_version, po.created_at, po.updated_at
             FROM purchase_orders po
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             LEFT JOIN external_companies ec ON ec.id = po.supplier_company_id
             WHERE po.id = ?",
            [purchase_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "purchase_orders".to_string(),
            id: purchase_order_id.to_string(),
        })?;
    map_purchase_order_row(row)
}

async fn get_goods_receipt(db: &DatabaseConnection, goods_receipt_id: i64) -> AppResult<GoodsReceipt> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, gr_number, purchase_order_id, status, posting_state, posting_error, received_by_id, received_at,
                    row_version, created_at, updated_at
             FROM goods_receipts WHERE id = ?",
            [goods_receipt_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "goods_receipts".to_string(),
            id: goods_receipt_id.to_string(),
        })?;
    map_goods_receipt_row(row)
}

pub async fn get_repairable_order(db: &DatabaseConnection, order_id: i64) -> AppResult<RepairableOrder> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{REPAIRABLE_ORDER_SELECT} WHERE ro.id = ?"),
            [order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "repairable_orders".to_string(),
            id: order_id.to_string(),
        })?;
    map_repairable_row(row)
}

fn map_requisition_row(row: sea_orm::QueryResult) -> AppResult<ProcurementRequisition> {
    Ok(ProcurementRequisition {
        id: row.try_get("", "id")?,
        req_number: row.try_get("", "req_number")?,
        demand_source_type: row.try_get("", "demand_source_type")?,
        demand_source_id: row.try_get("", "demand_source_id")?,
        demand_source_ref: row.try_get("", "demand_source_ref")?,
        purchase_priority: row
            .try_get("", "purchase_priority")
            .unwrap_or_else(|_| "NORMAL".to_string()),
        status: row.try_get("", "status")?,
        posting_state: row.try_get("", "posting_state")?,
        posting_error: row.try_get("", "posting_error")?,
        requested_by_id: row.try_get("", "requested_by_id")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_requisition_line_row(row: sea_orm::QueryResult) -> AppResult<ProcurementRequisitionLine> {
    Ok(ProcurementRequisitionLine {
        id: row.try_get("", "id")?,
        requisition_id: row.try_get("", "requisition_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        preferred_location_id: row.try_get("", "preferred_location_id")?,
        preferred_location_code: row.try_get("", "preferred_location_code")?,
        requested_qty: row.try_get("", "requested_qty")?,
        demand_source_line_id: row.try_get("", "demand_source_line_id").ok().flatten(),
        source_reservation_id: row.try_get("", "source_reservation_id")?,
        source_reorder_trigger: row.try_get("", "source_reorder_trigger")?,
        status: row.try_get("", "status")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn map_purchase_order_row(row: sea_orm::QueryResult) -> AppResult<PurchaseOrder> {
    Ok(PurchaseOrder {
        id: row.try_get("", "id")?,
        po_number: row.try_get("", "po_number")?,
        requisition_id: row.try_get("", "requisition_id")?,
        supplier_id: row.try_get("", "supplier_id").ok().flatten(),
        supplier_name: row.try_get("", "supplier_name").ok().flatten(),
        supplier_company_id: row.try_get("", "supplier_company_id")?,
        supplier_company_name: row.try_get("", "supplier_company_name")?,
        status: row.try_get("", "status")?,
        posting_state: row.try_get("", "posting_state")?,
        posting_error: row.try_get("", "posting_error")?,
        ordered_by_id: row.try_get("", "ordered_by_id")?,
        ordered_at: row.try_get("", "ordered_at")?,
        approved_by_id: row.try_get("", "approved_by_id")?,
        approved_at: row.try_get("", "approved_at")?,
        expected_delivery_date: row.try_get("", "expected_delivery_date").ok().flatten(),
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_po_line_row(row: sea_orm::QueryResult) -> AppResult<PurchaseOrderLine> {
    let ordered_qty: f64 = row.try_get("", "ordered_qty")?;
    let received_qty: f64 = row.try_get("", "received_qty")?;
    let unit_price: Option<f64> = row.try_get("", "unit_price")?;
    Ok(PurchaseOrderLine {
        id: row.try_get("", "id")?,
        purchase_order_id: row.try_get("", "purchase_order_id")?,
        requisition_line_id: row.try_get("", "requisition_line_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        ordered_qty,
        received_qty,
        unit_price,
        demand_source_type: row.try_get("", "demand_source_type")?,
        demand_source_id: row.try_get("", "demand_source_id")?,
        demand_source_ref: row.try_get("", "demand_source_ref")?,
        demand_source_line_id: row.try_get("", "demand_source_line_id").ok().flatten(),
        source_reservation_id: row.try_get("", "source_reservation_id")?,
        status: row.try_get("", "status")?,
        remaining_qty: (ordered_qty - received_qty).max(0.0),
        line_total: unit_price.map(|price| price * ordered_qty),
        work_order_id: row.try_get("", "work_order_id").ok().flatten(),
        work_order_code: row.try_get("", "work_order_code").ok().flatten(),
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_goods_receipt_row(row: sea_orm::QueryResult) -> AppResult<GoodsReceipt> {
    Ok(GoodsReceipt {
        id: row.try_get("", "id")?,
        gr_number: row.try_get("", "gr_number")?,
        purchase_order_id: row.try_get("", "purchase_order_id")?,
        status: row.try_get("", "status")?,
        posting_state: row.try_get("", "posting_state")?,
        posting_error: row.try_get("", "posting_error")?,
        received_by_id: row.try_get("", "received_by_id")?,
        received_at: row.try_get("", "received_at")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_goods_receipt_line_row(row: sea_orm::QueryResult) -> AppResult<GoodsReceiptLine> {
    Ok(GoodsReceiptLine {
        id: row.try_get("", "id")?,
        goods_receipt_id: row.try_get("", "goods_receipt_id")?,
        po_line_id: row.try_get("", "po_line_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        received_qty: row.try_get("", "received_qty")?,
        accepted_qty: row.try_get("", "accepted_qty")?,
        rejected_qty: row.try_get("", "rejected_qty")?,
        rejection_reason: row.try_get("", "rejection_reason")?,
        ordered_qty: row.try_get("", "ordered_qty").ok().flatten(),
        actual_lead_time_days: row.try_get("", "actual_lead_time_days").ok().flatten(),
        status: row.try_get("", "status")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn map_repairable_row(row: sea_orm::QueryResult) -> AppResult<RepairableOrder> {
    Ok(RepairableOrder {
        id: row.try_get("", "id")?,
        order_code: row.try_get("", "order_code")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        quantity: row.try_get("", "quantity")?,
        source_location_id: row.try_get("", "source_location_id")?,
        source_location_code: row.try_get("", "source_location_code")?,
        return_location_id: row.try_get("", "return_location_id")?,
        return_location_code: row.try_get("", "return_location_code")?,
        linked_po_line_id: row.try_get("", "linked_po_line_id")?,
        linked_reservation_id: row.try_get("", "linked_reservation_id")?,
        status: row.try_get("", "status")?,
        reason: row.try_get("", "reason")?,
        serial_number: row.try_get("", "serial_number").ok().flatten(),
        vendor_supplier_id: row.try_get("", "vendor_supplier_id").ok().flatten(),
        vendor_supplier_code: row.try_get("", "vendor_supplier_code").ok().flatten(),
        vendor_supplier_name: row.try_get("", "vendor_supplier_name").ok().flatten(),
        sent_at: row.try_get("", "sent_at").ok().flatten(),
        returned_at: row.try_get("", "returned_at").ok().flatten(),
        warranty_active: row.try_get("", "warranty_active").unwrap_or(0),
        warranty_until: row.try_get("", "warranty_until").ok().flatten(),
        repair_cost: row.try_get("", "repair_cost").ok().flatten(),
        work_order_id: row.try_get("", "work_order_id").ok().flatten(),
        work_order_code: row.try_get("", "work_order_code").ok().flatten(),
        created_by_id: row.try_get("", "created_by_id")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn check_wo_part_stock_availability(
    db: &DatabaseConnection,
    work_order_id: i64,
) -> AppResult<crate::inventory::domain::WoMaterialReadiness> {
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code FROM work_orders WHERE id = ?",
            [work_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "work_orders".to_string(),
            id: work_order_id.to_string(),
        })?;
    let work_order_code: String = wo_row.try_get("", "code")?;

    let parts = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wop.article_id,
                    COALESCE(a.article_code, wop.article_ref, '') AS article_code,
                    COALESCE(a.article_name, wop.article_ref, '') AS article_name,
                    wop.quantity_planned AS planned_qty,
                    COALESCE(wop.quantity_reserved, 0.0) AS quantity_reserved,
                    wop.stock_location_id AS preferred_location_id,
                    COALESCE(sb_sum.available_qty, 0.0) AS available_qty
             FROM work_order_parts wop
             LEFT JOIN articles a ON a.id = wop.article_id
             LEFT JOIN (
                SELECT article_id, SUM(available_qty) AS available_qty
                FROM stock_balances
                GROUP BY article_id
             ) sb_sum ON sb_sum.article_id = wop.article_id
             WHERE wop.work_order_id = ?
               AND wop.article_id IS NOT NULL
               AND COALESCE(wop.quantity_planned, 0) > 0",
            [work_order_id.into()],
        ))
        .await
        .unwrap_or_default();

    let total_parts = parts.len() as i64;
    let mut available_parts = 0i64;
    let mut reserved_parts = 0i64;
    let mut missing_parts = 0i64;
    let mut shortages = Vec::new();

    for part in &parts {
        let planned: f64 = part.try_get("", "planned_qty").unwrap_or(0.0);
        let available: f64 = part.try_get("", "available_qty").unwrap_or(0.0);
        let qty_reserved: f64 = part.try_get("", "quantity_reserved").unwrap_or(0.0);
        let covered = qty_reserved.max(available);
        if qty_reserved >= planned {
            reserved_parts += 1;
            available_parts += 1;
        } else if covered >= planned {
            available_parts += 1;
        } else {
            missing_parts += 1;
            let article_id: i64 = part.try_get("", "article_id").unwrap_or(0);
            shortages.push(crate::inventory::domain::WoPartShortage {
                article_id,
                article_code: part.try_get("", "article_code").unwrap_or_default(),
                article_name: part.try_get("", "article_name").unwrap_or_default(),
                requested_qty: planned,
                available_qty: available,
                shortage_qty: (planned - covered).max(0.0),
                preferred_location_id: part.try_get("", "preferred_location_id").ok().flatten(),
            });
        }
    }

    let ready_pct = if total_parts == 0 {
        100.0
    } else {
        (available_parts as f64 / total_parts as f64) * 100.0
    };
    let reserved_pct = if total_parts == 0 {
        0.0
    } else {
        (reserved_parts as f64 / total_parts as f64) * 100.0
    };

    let expected_arrival: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            // Earliest promised delivery across the WO-linked open POs. Order dates are not
            // arrival dates, so only POs carrying an expected_delivery_date can answer this.
            "SELECT MIN(po.expected_delivery_date) AS eta
             FROM purchase_orders po
             INNER JOIN purchase_order_lines pol ON pol.purchase_order_id = po.id
             WHERE pol.demand_source_type IN ('WORK_ORDER', 'WORK_ORDER_PART')
               AND pol.demand_source_id = ?
               AND po.status NOT IN ('RECEIVED_CLOSED', 'CANCELLED')
               AND po.expected_delivery_date IS NOT NULL",
            [work_order_id.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<Option<String>>("", "eta").ok().flatten());

    Ok(crate::inventory::domain::WoMaterialReadiness {
        work_order_id,
        work_order_code,
        total_parts,
        available_parts,
        reserved_parts,
        missing_parts,
        ready_pct,
        reserved_pct,
        expected_arrival,
        is_fully_available: missing_parts == 0,
        shortages,
    })
}

pub async fn create_procurement_requisition_from_wo_part(
    db: &DatabaseConnection,
    work_order_id: i64,
    article_id: i64,
    requested_qty: f64,
    preferred_location_id: Option<i64>,
    actor_id: Option<i64>,
) -> AppResult<ProcurementRequisition> {
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM work_orders WHERE id = ?",
            [work_order_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "work_orders".to_string(),
            id: work_order_id.to_string(),
        })?;
    let wo_number: String = wo_row.try_get("", "code")?;

    create_procurement_requisition(
        db,
        CreateProcurementRequisitionInput {
            article_id,
            preferred_location_id,
            requested_qty,
            demand_source_type: "WORK_ORDER_PART".to_string(),
            demand_source_id: Some(work_order_id),
            demand_source_ref: Some(wo_number),
            demand_source_line_id: None,
            source_reservation_id: None,
            source_reorder_trigger: None,
            purchase_priority: Some("HIGH".to_string()),
            reason: Some("Auto-created from WO part shortage".to_string()),
            actor_id,
        },
    )
    .await
}

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    CreateProcurementRequisitionInput, CreatePurchaseOrderFromRequisitionInput, CreateRepairableOrderInput, GoodsReceipt,
    GoodsReceiptLine, InventoryStateEvent, ProcurementRequisition, ProcurementRequisitionLine, ProcurementSupplier,
    PurchaseOrder, PurchaseOrderLine, ReceiveGoodsInput, RepairableOrder, TransitionProcurementRequisitionInput,
    TransitionPurchaseOrderInput, TransitionRepairableOrderInput, UpdatePostingStateInput,
};

const PROC_STATUS_DOMAIN: &str = "inventory.procurement_status";
const ERP_POSTING_DOMAIN: &str = "inventory.erp_posting_state";
const REPAIRABLE_STATUS_DOMAIN: &str = "inventory.repairable_status";

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn next_doc_number(prefix: &str) -> String {
    format!("{prefix}-{}", Utc::now().timestamp_millis())
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
            "SELECT is_active FROM external_companies WHERE id = ?",
            [supplier_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "external_companies".to_string(),
            id: supplier_id.to_string(),
        });
    };
    let is_active: i64 = row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Selected supplier is inactive.".to_string(),
        ]));
    }
    Ok(())
}

async fn record_state_event<C: ConnectionTrait>(
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
            "SELECT id,
                    printf('SUP-%05d', id) AS company_code,
                    name AS company_name,
                    is_active
             FROM external_companies
             ORDER BY company_name ASC"
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
            (req_number, demand_source_type, demand_source_id, demand_source_ref, status, posting_state, requested_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'DRAFT', 'PENDING_POSTING', ?, ?, ?)",
        [
            req_number.clone().into(),
            input.demand_source_type.into(),
            input.demand_source_id.map_or(Value::BigInt(None), Value::from),
            input.demand_source_ref.clone().map_or(Value::String(None), Value::from),
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
            (requisition_id, article_id, preferred_location_id, requested_qty, source_reservation_id, source_reorder_trigger, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, 'OPEN', ?)",
        [
            req_id.into(),
            input.article_id.into(),
            input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
            input.requested_qty.into(),
            input.source_reservation_id.map_or(Value::BigInt(None), Value::from),
            input.source_reorder_trigger.clone().map_or(Value::String(None), Value::from),
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
            "Requisition row_version mismatch.".to_string(),
        ]));
    }
    guard_transition(
        &current,
        &input.next_status,
        &[
            ("DRAFT", &["SUBMITTED", "CANCELLED"]),
            ("SUBMITTED", &["APPROVED", "CANCELLED"]),
            ("APPROVED", &["CLOSED", "CANCELLED"]),
            ("PARTIALLY_RECEIVED", &["CLOSED", "CANCELLED"]),
            ("CLOSED", &[]),
            ("CANCELLED", &[]),
        ],
        "requisition",
    )?;

    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE procurement_requisitions
         SET status = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [input.next_status.clone().into(), now_iso().into(), input.requisition_id.into()],
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
    if let Some(supplier_id) = input.supplier_company_id {
        ensure_supplier_active(db, supplier_id).await?;
    }
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
            (po_number, requisition_id, supplier_company_id, status, posting_state, ordered_by_id, ordered_at, created_at, updated_at)
         VALUES (?, ?, ?, 'DRAFT', 'PENDING_POSTING', ?, ?, ?, ?)",
        [
            po_number.clone().into(),
            input.requisition_id.into(),
            input.supplier_company_id.map_or(Value::BigInt(None), Value::from),
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

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO purchase_order_lines
         (purchase_order_id, requisition_line_id, article_id, ordered_qty, received_qty, unit_price, demand_source_type,
          demand_source_id, demand_source_ref, source_reservation_id, status, created_at, updated_at)
         SELECT ?, rl.id, rl.article_id, rl.requested_qty, 0, NULL,
                r.demand_source_type, r.demand_source_id, r.demand_source_ref, rl.source_reservation_id,
                'OPEN', ?, ?
         FROM procurement_requisition_lines rl
         JOIN procurement_requisitions r ON r.id = rl.requisition_id
         WHERE rl.requisition_id = ?",
        [po_id.into(), now_iso().into(), now_iso().into(), input.requisition_id.into()],
    ))
    .await?;
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
        return Err(AppError::ValidationFailed(vec!["Purchase order row_version mismatch.".to_string()]));
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
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE purchase_orders
         SET status = ?, row_version = row_version + 1, updated_at = ?,
             approved_at = CASE WHEN ? = 'APPROVED' THEN ? ELSE approved_at END,
             approved_by_id = CASE WHEN ? = 'APPROVED' THEN ? ELSE approved_by_id END
         WHERE id = ?",
        [
            input.next_status.clone().into(),
            now_iso().into(),
            input.next_status.clone().into(),
            now_iso().into(),
            input.next_status.clone().into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
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

pub async fn receive_purchase_order_goods(db: &DatabaseConnection, input: ReceiveGoodsInput) -> AppResult<GoodsReceipt> {
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
             (goods_receipt_id, po_line_id, article_id, location_id, received_qty, accepted_qty, rejected_qty, rejection_reason, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'RECEIVED', ?)",
            [
                gr_id.into(),
                line.po_line_id.into(),
                line.article_id.into(),
                line.location_id.into(),
                line.received_qty.into(),
                line.accepted_qty.into(),
                line.rejected_qty.into(),
                line.rejection_reason.clone().map_or(Value::String(None), Value::from),
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

pub async fn create_repairable_order(db: &DatabaseConnection, input: CreateRepairableOrderInput) -> AppResult<RepairableOrder> {
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
    let code = next_doc_number("REP");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO repairable_orders
            (order_code, article_id, quantity, source_location_id, return_location_id, linked_po_line_id, linked_reservation_id,
             status, reason, created_by_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'REQUESTED', ?, ?, ?, ?)",
        [
            code.clone().into(),
            input.article_id.into(),
            input.quantity.into(),
            input.source_location_id.into(),
            input.return_location_id.map_or(Value::BigInt(None), Value::from),
            input.linked_po_line_id.map_or(Value::BigInt(None), Value::from),
            input.linked_reservation_id.map_or(Value::BigInt(None), Value::from),
            input.reason.clone().map_or(Value::String(None), Value::from),
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

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE repairable_orders
         SET status = ?, return_location_id = COALESCE(?, return_location_id),
             row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.next_status.clone().into(),
            input.return_location_id.map_or(Value::BigInt(None), Value::from),
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
                    rl.source_reservation_id, rl.source_reorder_trigger, rl.status, rl.created_at
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
            "SELECT po.id, po.po_number, po.requisition_id, po.supplier_company_id, ec.name AS supplier_company_name,
                    po.status, po.posting_state, po.posting_error, po.ordered_by_id, po.ordered_at, po.approved_by_id,
                    po.approved_at, po.row_version, po.created_at, po.updated_at
             FROM purchase_orders po
             LEFT JOIN external_companies ec ON ec.id = po.supplier_company_id
             ORDER BY po.id DESC"
            .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_purchase_order_row).collect()
}

pub async fn list_purchase_order_lines(db: &DatabaseConnection, purchase_order_id: i64) -> AppResult<Vec<PurchaseOrderLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pol.id, pol.purchase_order_id, pol.requisition_line_id, pol.article_id, a.article_code, a.article_name,
                    pol.ordered_qty, pol.received_qty, pol.unit_price, pol.demand_source_type, pol.demand_source_id,
                    pol.demand_source_ref, pol.source_reservation_id, pol.status, pol.created_at, pol.updated_at
             FROM purchase_order_lines pol
             JOIN articles a ON a.id = pol.article_id
             WHERE pol.purchase_order_id = ?
             ORDER BY pol.id ASC",
            [purchase_order_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_po_line_row).collect()
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

pub async fn list_goods_receipt_lines(db: &DatabaseConnection, goods_receipt_id: i64) -> AppResult<Vec<GoodsReceiptLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT grl.id, grl.goods_receipt_id, grl.po_line_id, grl.article_id, a.article_code, a.article_name,
                    grl.location_id, sl.code AS location_code, grl.received_qty, grl.accepted_qty, grl.rejected_qty,
                    grl.rejection_reason, grl.status, grl.created_at
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

pub async fn list_repairable_orders(db: &DatabaseConnection) -> AppResult<Vec<RepairableOrder>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ro.id, ro.order_code, ro.article_id, a.article_code, a.article_name, ro.quantity,
                    ro.source_location_id, src.code AS source_location_code, ro.return_location_id,
                    ret.code AS return_location_code, ro.linked_po_line_id, ro.linked_reservation_id, ro.status,
                    ro.reason, ro.created_by_id, ro.row_version, ro.created_at, ro.updated_at
             FROM repairable_orders ro
             JOIN articles a ON a.id = ro.article_id
             JOIN stock_locations src ON src.id = ro.source_location_id
             LEFT JOIN stock_locations ret ON ret.id = ro.return_location_id
             ORDER BY ro.id DESC"
            .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_repairable_row).collect()
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

async fn get_procurement_requisition(db: &DatabaseConnection, requisition_id: i64) -> AppResult<ProcurementRequisition> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, req_number, demand_source_type, demand_source_id, demand_source_ref,
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
            "SELECT po.id, po.po_number, po.requisition_id, po.supplier_company_id, ec.name AS supplier_company_name,
                    po.status, po.posting_state, po.posting_error, po.ordered_by_id, po.ordered_at, po.approved_by_id,
                    po.approved_at, po.row_version, po.created_at, po.updated_at
             FROM purchase_orders po
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

async fn get_repairable_order(db: &DatabaseConnection, order_id: i64) -> AppResult<RepairableOrder> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ro.id, ro.order_code, ro.article_id, a.article_code, a.article_name, ro.quantity,
                    ro.source_location_id, src.code AS source_location_code, ro.return_location_id,
                    ret.code AS return_location_code, ro.linked_po_line_id, ro.linked_reservation_id, ro.status,
                    ro.reason, ro.created_by_id, ro.row_version, ro.created_at, ro.updated_at
             FROM repairable_orders ro
             JOIN articles a ON a.id = ro.article_id
             JOIN stock_locations src ON src.id = ro.source_location_id
             LEFT JOIN stock_locations ret ON ret.id = ro.return_location_id
             WHERE ro.id = ?",
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
        supplier_company_id: row.try_get("", "supplier_company_id")?,
        supplier_company_name: row.try_get("", "supplier_company_name")?,
        status: row.try_get("", "status")?,
        posting_state: row.try_get("", "posting_state")?,
        posting_error: row.try_get("", "posting_error")?,
        ordered_by_id: row.try_get("", "ordered_by_id")?,
        ordered_at: row.try_get("", "ordered_at")?,
        approved_by_id: row.try_get("", "approved_by_id")?,
        approved_at: row.try_get("", "approved_at")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_po_line_row(row: sea_orm::QueryResult) -> AppResult<PurchaseOrderLine> {
    Ok(PurchaseOrderLine {
        id: row.try_get("", "id")?,
        purchase_order_id: row.try_get("", "purchase_order_id")?,
        requisition_line_id: row.try_get("", "requisition_line_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        ordered_qty: row.try_get("", "ordered_qty")?,
        received_qty: row.try_get("", "received_qty")?,
        unit_price: row.try_get("", "unit_price")?,
        demand_source_type: row.try_get("", "demand_source_type")?,
        demand_source_id: row.try_get("", "demand_source_id")?,
        demand_source_ref: row.try_get("", "demand_source_ref")?,
        source_reservation_id: row.try_get("", "source_reservation_id")?,
        status: row.try_get("", "status")?,
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
        created_by_id: row.try_get("", "created_by_id")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

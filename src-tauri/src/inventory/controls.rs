use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, InventoryCountLine, InventoryCountSession,
    InventoryReconciliationFinding, InventoryReconciliationRun, PostInventoryCountSessionInput,
    ReverseInventoryCountSessionInput, RunInventoryReconciliationInput, TransitionInventoryCountSessionInput,
    UpsertInventoryCountLineInput,
};

/// Refusal code: a count of the same article and location was posted on this PC since the session
/// was opened (its snapshot is stale).
pub const REFUSED_COUNT_SESSION_STALE: &str = "COUNT_SESSION_STALE_SNAPSHOT";

/// Tenant setting (catalog, decimal): a counted line whose variance is at least this much is
/// critical. Each count session keeps the value it was opened with.
pub const SETTING_COUNT_CRITICAL_VARIANCE: &str = "inventory.count_critical_variance";
/// Tenant setting (catalog, decimal): a ledger / balance gap of at least this much is a
/// reconciliation break.
pub const SETTING_RECONCILIATION_DRIFT_BREAK: &str = "inventory.reconciliation_drift_break";

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn next_doc(prefix: &str) -> String {
    format!("{prefix}-{}", Utc::now().timestamp_millis())
}

async fn ensure_count_session(db: &DatabaseConnection, session_id: i64) -> AppResult<InventoryCountSession> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, session_code, warehouse_id, location_id, status, critical_abs_threshold,
                    submitted_by_id, submitted_at, posted_by_id, posted_at, reversed_by_id, reversed_at,
                    reversal_reason, row_version, created_at, updated_at
             FROM inventory_count_sessions
             WHERE id = ?",
            [session_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_count_sessions".to_string(),
            id: session_id.to_string(),
        })?;
    map_count_session(row)
}

async fn ensure_lookup_variance_reason_active<C: ConnectionTrait>(db: &C, code: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.variance_reason'
               AND lv.code = ?
               AND lv.is_active = 1
               AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "variance_reason_code '{code}' is not active."
        )]));
    }
    Ok(())
}

async fn get_balance<C: ConnectionTrait>(db: &C, article_id: i64, location_id: i64) -> AppResult<(i64, f64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT warehouse_id, on_hand_qty
             FROM stock_balances
             WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    if let Some(row) = row {
        Ok((row.try_get("", "warehouse_id")?, row.try_get("", "on_hand_qty")?))
    } else {
        let wh_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT warehouse_id FROM stock_locations WHERE id = ?",
                [location_id.into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "GATE_INV_LOCATION_UNKNOWN: location_id does not exist.".to_string()
                ])
            })?;
        Ok((wh_row.try_get("", "warehouse_id")?, 0.0))
    }
}

/// Sets the counted on-hand stock of a location; its reservations are kept, so the available stock
/// is the new on-hand minus what is reserved there.
async fn upsert_balance<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    on_hand_qty: f64,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_balances (article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at)
         VALUES (?, ?, ?, ?, 0, ?, ?)
         ON CONFLICT(article_id, location_id) DO UPDATE SET
            on_hand_qty = excluded.on_hand_qty,
            available_qty = excluded.on_hand_qty - stock_balances.reserved_qty,
            updated_at = excluded.updated_at",
        [
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            on_hand_qty.into(),
            on_hand_qty.into(),
            now_iso().into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn insert_count_transaction<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    qty: f64,
    movement_type: &str,
    session_id: i64,
    source_ref: &str,
    reason: Option<&str>,
    actor_id: Option<i64>,
) -> AppResult<i64> {
    // Count postings and reversals are dated today; refused when today falls in a closed period.
    crate::inventory::periods::ensure_posting_date_open(db, &crate::inventory::fx::today()).await?;
    let entity_sync_id = uuid::Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions
            (entity_sync_id, article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason, performed_by_id, performed_at, row_version)
         VALUES (?, ?, ?, ?, NULL, ?, ?, 'COUNT_SESSION', ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            movement_type.into(),
            qty.into(),
            session_id.into(),
            source_ref.into(),
            reason.map_or(Value::String(None), Value::from),
            actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
        ],
    ))
    .await?;
    let tx_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Could not read transaction id.".to_string()]))?;
    Ok(tx_row.try_get("", "id")?)
}

pub async fn create_count_session(
    db: &DatabaseConnection,
    input: CreateInventoryCountSessionInput,
) -> AppResult<InventoryCountSession> {
    // The tolerance is the tenant setting (Settings > Inventory), kept on the session.
    let critical_abs_threshold = crate::settings::catalog::decimal_setting(db, SETTING_COUNT_CRITICAL_VARIANCE).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_sessions
            (session_code, warehouse_id, location_id, status, critical_abs_threshold, opened_by_id, created_at, updated_at)
         VALUES (?, ?, ?, 'draft', ?, ?, ?, ?)",
        [
            next_doc("CC").into(),
            input.warehouse_id.into(),
            input.location_id.map_or(Value::BigInt(None), Value::from),
            critical_abs_threshold.into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            now_iso().into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Session creation failed.".to_string()]))?;
    ensure_count_session(db, row.try_get("", "id")?).await
}

pub async fn transition_count_session(
    db: &DatabaseConnection,
    input: TransitionInventoryCountSessionInput,
) -> AppResult<InventoryCountSession> {
    let current = ensure_count_session(db, input.session_id).await?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: count session row_version mismatch.".to_string(),
        ]));
    }
    let allowed = match current.status.as_str() {
        "draft" => ["counting", "cancelled"].as_slice(),
        "counting" => ["submitted", "cancelled"].as_slice(),
        "submitted" => ["approved", "cancelled"].as_slice(),
        "approved" => ["posted", "cancelled"].as_slice(),
        "posted" => ["reversed"].as_slice(),
        _ => [].as_slice(),
    };
    if !allowed.iter().any(|x| *x == input.next_status) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Invalid count-session transition: {} -> {}",
            current.status, input.next_status
        )]));
    }
    let next_status = input.next_status.clone();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions
         SET status = ?, row_version = row_version + 1, submitted_by_id = CASE WHEN ? = 'submitted' THEN ? ELSE submitted_by_id END,
             submitted_at = CASE WHEN ? = 'submitted' THEN ? ELSE submitted_at END,
             updated_at = ?
         WHERE id = ?",
        [
            next_status.clone().into(),
            next_status.clone().into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            next_status.into(),
            now_iso().into(),
            now_iso().into(),
            input.session_id.into(),
        ],
    ))
    .await?;
    ensure_count_session(db, input.session_id).await
}

pub async fn upsert_count_line(
    db: &DatabaseConnection,
    input: UpsertInventoryCountLineInput,
) -> AppResult<InventoryCountLine> {
    if input.counted_qty < 0.0 {
        return Err(AppError::ValidationFailed(
            vec!["counted_qty must be >= 0.".to_string()],
        ));
    }
    let session = ensure_count_session(db, input.session_id).await?;
    if session.status != "draft" && session.status != "counting" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_NOT_EDITABLE: Count lines can only be edited in draft/counting status.".to_string(),
        ]));
    }
    // The stock the count is set against and the counts it knows are read in one transaction (one
    // snapshot): a sync pass in between can never make the count know a correction its stock does
    // not include (`negative_stock::count_mark` also follows only counts whose movement is here).
    let tx = db.begin().await?;
    let (warehouse_id, system_qty) = get_balance(&tx, input.article_id, input.location_id).await?;
    if warehouse_id != session.warehouse_id {
        return Err(AppError::ValidationFailed(vec![
            "Count line location must belong to session warehouse.".to_string(),
        ]));
    }
    let variance_qty = input.counted_qty - system_qty;
    if variance_qty.abs() > f64::EPSILON && input.variance_reason_code.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_VARIANCE_REASON: variance_reason_code is mandatory when variance exists.".to_string(),
        ]));
    }
    if let Some(code) = input.variance_reason_code.as_deref() {
        ensure_lookup_variance_reason_active(&tx, code).await?;
    }
    let is_critical = i64::from(variance_qty.abs() >= session.critical_abs_threshold);
    let approval_required = i64::from(variance_qty.abs() > f64::EPSILON);
    // The count's mark is taken now, with the stock on record its variance is set against: the
    // counts this PC knows at this moment, not at posting, so a count received in between still
    // shows as a double count (`inventory::negative_stock`, CMMS Advisor). Each entry refreshes it.
    let (replica_id, follows_json) =
        crate::inventory::negative_stock::count_mark(&tx, input.article_id, input.location_id).await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_lines
            (session_id, article_id, warehouse_id, location_id, system_qty, counted_qty, variance_qty,
             variance_reason_code, is_critical, approval_required, created_at, updated_at,
             count_replica_id, count_follows_json, count_path)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(session_id, article_id, location_id) DO UPDATE SET
             counted_qty = excluded.counted_qty,
             system_qty = excluded.system_qty,
             variance_qty = excluded.variance_qty,
             variance_reason_code = excluded.variance_reason_code,
             is_critical = excluded.is_critical,
             approval_required = excluded.approval_required,
             approved_by_id = NULL,
             approved_at = NULL,
             approval_note = NULL,
             count_replica_id = excluded.count_replica_id,
             count_follows_json = excluded.count_follows_json,
             count_path = excluded.count_path,
             row_version = inventory_count_lines.row_version + 1,
             updated_at = excluded.updated_at",
        [
            input.session_id.into(),
            input.article_id.into(),
            warehouse_id.into(),
            input.location_id.into(),
            system_qty.into(),
            input.counted_qty.into(),
            variance_qty.into(),
            input.variance_reason_code.map_or(Value::String(None), Value::from),
            is_critical.into(),
            approval_required.into(),
            now_iso().into(),
            now_iso().into(),
            replica_id.into(),
            follows_json.into(),
            crate::inventory::negative_stock::COUNT_PATH_COUNT_SESSION.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.id
             FROM inventory_count_lines cl
             WHERE cl.session_id = ? AND cl.article_id = ? AND cl.location_id = ?",
            [
                input.session_id.into(),
                input.article_id.into(),
                input.location_id.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to upsert count line.".to_string()]))?;
    get_count_line(db, row.try_get("", "id")?).await
}

pub async fn approve_count_line(
    db: &DatabaseConnection,
    input: ApproveInventoryCountLineInput,
) -> AppResult<InventoryCountLine> {
    if input.reviewer_evidence.trim().chars().count() < crate::inventory::negative_stock::REVIEW_REASON_MIN_CHARS {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_EVIDENCE: reviewer_evidence must be explicit and at least 8 characters.".to_string(),
        ]));
    }
    let line = get_count_line(db, input.line_id).await?;
    if line.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: count line row_version mismatch.".to_string(),
        ]));
    }
    if line.approval_required == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Approval is not required for zero variance lines.".to_string(),
        ]));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_lines
         SET approved_by_id = ?, approved_at = ?, approval_note = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.reviewer_id.into(),
            now_iso().into(),
            input.reviewer_evidence.into(),
            now_iso().into(),
            input.line_id.into(),
        ],
    ))
    .await?;
    get_count_line(db, input.line_id).await
}

pub async fn post_count_session(
    db: &DatabaseConnection,
    input: PostInventoryCountSessionInput,
) -> AppResult<InventoryCountSession> {
    let session = ensure_count_session(db, input.session_id).await?;
    if session.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: count session row_version mismatch.".to_string(),
        ]));
    }
    if session.status != "approved" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_NOT_APPROVED: Count session must be approved before posting.".to_string(),
        ]));
    }
    let lines = list_count_lines(db, input.session_id).await?;
    if lines.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_NO_LINES: Count session has no lines.".to_string(),
        ]));
    }
    for line in &lines {
        if line.variance_qty.abs() > f64::EPSILON {
            if line.variance_reason_code.is_none() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Line {} has variance but no variance reason.",
                    line.id
                )]));
            }
            if line.approved_by_id.is_none() || line.approval_note.as_deref().unwrap_or("").trim().is_empty() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "GATE_INV_COUNT_EVIDENCE: Line {} cannot post without reviewer evidence.",
                    line.id
                )]));
            }
        }
    }

    let tx = db.begin().await?;
    // A count posted on this PC for the same article and location since this session was opened
    // (times to the second; within the same second, a session opened after this one) makes its
    // snapshot stale: its variance would correct the stock a second time. Refused; the
    // session is counted again from a fresh snapshot (CMMS Advisor). Counts from other PCs are not
    // refused here: they arrive later and show as a double count (`inventory::negative_stock`).
    let replica_id = crate::sync::queries::local_replica_id(&tx).await?;
    for line in &lines {
        let stale = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT cs.session_code FROM inventory_count_lines cl
                 JOIN inventory_count_sessions cs ON cs.id = cl.session_id
                 WHERE cl.article_id = ? AND cl.location_id = ? AND cl.session_id <> ?
                   AND cs.status IN ('posted', 'reversed') AND cl.count_replica_id = ?
                   AND (cs.posted_at > ? OR (cs.posted_at = ? AND cs.id > ?))
                 ORDER BY cs.posted_at DESC LIMIT 1",
                [
                    line.article_id.into(),
                    line.location_id.into(),
                    input.session_id.into(),
                    replica_id.clone().into(),
                    session.created_at.clone().into(),
                    session.created_at.clone().into(),
                    input.session_id.into(),
                ],
            ))
            .await?;
        if let Some(row) = stale {
            let other: String = row.try_get("", "session_code")?;
            return Err(crate::errors::org_validation_failed(
                REFUSED_COUNT_SESSION_STALE,
                format!(
                    "A count was posted for {} at {} since this session was opened ({other}): cancel this session and count again in a new session.",
                    line.article_code, line.location_code
                ),
                crate::errors::issue_params(&[
                    ("article", line.article_code.clone()),
                    ("location", line.location_code.clone()),
                    ("session", other),
                ]),
            ));
        }
    }
    for line in lines {
        // Every posted line is a count of its pair and keeps the mark taken when its quantity was
        // entered (`upsert_count_line`). A line entered before this version has none: it is marked
        // now, with what this PC knows at posting (`inventory::negative_stock`, CMMS Advisor).
        let unmarked = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS unmarked FROM inventory_count_lines WHERE id = ? AND count_follows_json IS NULL",
                [line.id.into()],
            ))
            .await?
            .is_some();
        if unmarked {
            let (replica_id, follows_json) =
                crate::inventory::negative_stock::count_mark(&tx, line.article_id, line.location_id).await?;
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inventory_count_lines SET count_replica_id = ?, count_follows_json = ?, count_path = ?,
                     row_version = row_version + 1, updated_at = ? WHERE id = ?",
                [
                    replica_id.into(),
                    follows_json.into(),
                    crate::inventory::negative_stock::COUNT_PATH_COUNT_SESSION.into(),
                    now_iso().into(),
                    line.id.into(),
                ],
            ))
            .await?;
        }
        if line.variance_qty.abs() <= f64::EPSILON {
            continue;
        }
        let (_, current_qty) = get_balance(&tx, line.article_id, line.location_id).await?;
        let next_qty = current_qty + line.variance_qty;
        if next_qty < 0.0 {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_INV_NEGATIVE_STOCK: Posting would produce negative stock for line {}.",
                line.id
            )]));
        }
        upsert_balance(&tx, line.article_id, line.warehouse_id, line.location_id, next_qty).await?;
        let tx_id = insert_count_transaction(
            &tx,
            line.article_id,
            line.warehouse_id,
            line.location_id,
            line.variance_qty,
            "COUNT_ADJUST",
            input.session_id,
            &session.session_code,
            line.variance_reason_code.as_deref(),
            input.actor_id,
        )
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines SET posted_transaction_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [tx_id.into(), now_iso().into(), line.id.into()],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_mutation_audit_links
                (transaction_id, source_entity_type, source_entity_id, reviewer_id, reviewer_evidence, created_at)
             VALUES (?, 'inventory_count_line', ?, ?, ?, ?)",
            [
                tx_id.into(),
                line.id.into(),
                line.approved_by_id.map_or(Value::BigInt(None), Value::from),
                line.approval_note.map_or(Value::String(None), Value::from),
                now_iso().into(),
            ],
        ))
        .await?;
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions
         SET status = 'posted', posted_by_id = ?, posted_at = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            now_iso().into(),
            input.session_id.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    ensure_count_session(db, input.session_id).await
}

pub async fn reverse_count_session(
    db: &DatabaseConnection,
    input: ReverseInventoryCountSessionInput,
) -> AppResult<InventoryCountSession> {
    // Like a count's approval: a reversal can bring a location below zero back to zero or more,
    // which closes its item as an adjustment (`inventory::negative_stock`).
    if input.reason.trim().chars().count() < crate::inventory::negative_stock::REVIEW_REASON_MIN_CHARS {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INV_REVERSAL_REASON: Reversal reason must be explicit (at least {} characters).",
            crate::inventory::negative_stock::REVIEW_REASON_MIN_CHARS
        )]));
    }
    let session = ensure_count_session(db, input.session_id).await?;
    if session.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: count session row_version mismatch.".to_string(),
        ]));
    }
    if session.status != "posted" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_NOT_POSTED: Only posted count sessions can be reversed.".to_string(),
        ]));
    }
    let lines = list_count_lines(db, input.session_id).await?;
    let tx = db.begin().await?;
    for line in lines {
        if line.posted_transaction_id.is_none() || line.variance_qty.abs() <= f64::EPSILON {
            continue;
        }
        let (_, current_qty) = get_balance(&tx, line.article_id, line.location_id).await?;
        let next_qty = current_qty - line.variance_qty;
        // A reversal never takes stock below zero, but one that raises a location already below
        // zero is accepted even if it stays there (CMMS Advisor): its item stays open until the
        // stock is zero or more, and it is audited.
        let improves_negative =
            current_qty < -crate::inventory::negative_stock::QTY_TOLERANCE && next_qty > current_qty;
        if next_qty < 0.0 && !improves_negative {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_INV_NEGATIVE_STOCK: Reversal would produce negative stock for line {}.",
                line.id
            )]));
        }
        upsert_balance(&tx, line.article_id, line.warehouse_id, line.location_id, next_qty).await?;
        let reverse_tx_id = insert_count_transaction(
            &tx,
            line.article_id,
            line.warehouse_id,
            line.location_id,
            -line.variance_qty,
            "COUNT_REVERSAL",
            input.session_id,
            &session.session_code,
            Some(&input.reason),
            input.actor_id,
        )
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines SET reversed_transaction_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [reverse_tx_id.into(), now_iso().into(), line.id.into()],
        ))
        .await?;
        if improves_negative {
            crate::inventory::negative_stock::reconcile_pair_in(&tx, line.article_id, line.location_id).await?;
            crate::audit::writer::write_audit_event_in(
                &tx,
                crate::audit::writer::AuditEventInput {
                    action_code: "inventory.negative_stock.count_reversed".to_string(),
                    target_type: Some("stock_location_article".to_string()),
                    target_id: Some(format!("{}:{}", line.article_id, line.location_id)),
                    actor_id: input.actor_id,
                    auth_context: "session".to_string(),
                    result: "success".to_string(),
                    before_hash: None,
                    after_hash: None,
                    retention_class: "standard".to_string(),
                    details_json: Some(serde_json::json!({
                        "summary": "Count reversed at a location below zero",
                        "article_id": line.article_id,
                        "location_id": line.location_id,
                        "on_hand_before": current_qty,
                        "on_hand_after": next_qty,
                        "count_session_id": input.session_id,
                        "count_session_code": session.session_code,
                        "count_line_id": line.id,
                        "transaction_id": reverse_tx_id,
                        "reason": input.reason.trim(),
                    })),
                },
            )
            .await?;
        }
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions
         SET status = 'reversed', reversed_by_id = ?, reversed_at = ?, reversal_reason = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ?",
        [
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
            input.reason.into(),
            now_iso().into(),
            input.session_id.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    ensure_count_session(db, input.session_id).await
}

/// One location counted for one article, recorded and posted at once by the person counting.
pub(crate) struct LocationCount<'a> {
    pub article_id: i64,
    pub location_id: i64,
    pub counted_qty: f64,
    /// Code of the plant's `inventory.variance_reason` list (required when the count differs).
    pub variance_reason_code: &'a str,
    /// Why the count was made: kept as the line's reviewer evidence and on the movement's audit link.
    pub evidence: &'a str,
    pub actor_id: i64,
    /// The count's mark (`negative_stock::count_mark`): this PC's sync replica id and the JSON list
    /// of the counts of the pair it follows.
    pub mark: (&'a str, &'a str),
}

/// What [`record_location_count_in`] wrote.
pub(crate) struct PostedLocationCount {
    pub session_id: i64,
    pub session_code: String,
    pub line_id: i64,
    pub system_qty: f64,
    pub variance_qty: f64,
    /// The count movement (none when the count equals the stock on record).
    pub transaction_id: Option<i64>,
}

/// Records a count of one article at one location as a posted count session, in the caller's
/// transaction: a session of that location with one line, counted, approved (the evidence is the
/// approval note) and posted by the same person, the `COUNT_ADJUST` movement of the variance and
/// its audit link — the rows of the count workflow, so the count reaches the other PCs and shows in
/// the count history like any posted count. Used where a count is the resolution of a review
/// (`inventory::negative_stock`); the caller checks the right and the reason.
pub(crate) async fn record_location_count_in<C: ConnectionTrait>(
    tx: &C,
    count: &LocationCount<'_>,
) -> AppResult<PostedLocationCount> {
    if !count.counted_qty.is_finite() || count.counted_qty < 0.0 {
        return Err(AppError::ValidationFailed(
            vec!["counted_qty must be >= 0.".to_string()],
        ));
    }
    let (warehouse_id, system_qty) = get_balance(tx, count.article_id, count.location_id).await?;
    let variance_qty = count.counted_qty - system_qty;
    let has_variance = variance_qty.abs() > f64::EPSILON;
    if has_variance {
        ensure_lookup_variance_reason_active(tx, count.variance_reason_code).await?;
    }
    let critical_abs_threshold = crate::settings::catalog::decimal_setting(tx, SETTING_COUNT_CRITICAL_VARIANCE).await?;
    let now = now_iso();
    let session_code = next_doc("CC");
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_sessions
            (session_code, warehouse_id, location_id, status, critical_abs_threshold, opened_by_id,
             submitted_by_id, submitted_at, posted_by_id, posted_at, created_at, updated_at)
         VALUES (?, ?, ?, 'posted', ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            session_code.clone().into(),
            warehouse_id.into(),
            count.location_id.into(),
            critical_abs_threshold.into(),
            count.actor_id.into(),
            count.actor_id.into(),
            now.clone().into(),
            count.actor_id.into(),
            now.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;
    let session_id = last_insert_id(tx).await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_lines
            (session_id, article_id, warehouse_id, location_id, system_qty, counted_qty, variance_qty,
             variance_reason_code, is_critical, approval_required, approved_by_id, approved_at, approval_note,
             created_at, updated_at, count_replica_id, count_follows_json, count_path)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            session_id.into(),
            count.article_id.into(),
            warehouse_id.into(),
            count.location_id.into(),
            system_qty.into(),
            count.counted_qty.into(),
            variance_qty.into(),
            if has_variance {
                Value::from(count.variance_reason_code)
            } else {
                Value::String(None)
            },
            i64::from(variance_qty.abs() >= critical_abs_threshold).into(),
            i64::from(has_variance).into(),
            count.actor_id.into(),
            now.clone().into(),
            count.evidence.into(),
            now.clone().into(),
            now.clone().into(),
            count.mark.0.into(),
            count.mark.1.into(),
            crate::inventory::negative_stock::COUNT_PATH_RECONCILIATION.into(),
        ],
    ))
    .await?;
    let line_id = last_insert_id(tx).await?;
    let mut transaction_id = None;
    if has_variance {
        upsert_balance(tx, count.article_id, warehouse_id, count.location_id, count.counted_qty).await?;
        let tx_id = insert_count_transaction(
            tx,
            count.article_id,
            warehouse_id,
            count.location_id,
            variance_qty,
            "COUNT_ADJUST",
            session_id,
            &session_code,
            Some(count.variance_reason_code),
            Some(count.actor_id),
        )
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines SET posted_transaction_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [tx_id.into(), now.clone().into(), line_id.into()],
        ))
        .await?;
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_mutation_audit_links
                (transaction_id, source_entity_type, source_entity_id, reviewer_id, reviewer_evidence, created_at)
             VALUES (?, 'inventory_count_line', ?, ?, ?, ?)",
            [
                tx_id.into(),
                line_id.into(),
                count.actor_id.into(),
                count.evidence.into(),
                now.clone().into(),
            ],
        ))
        .await?;
        transaction_id = Some(tx_id);
    }
    Ok(PostedLocationCount {
        session_id,
        session_code,
        line_id,
        system_qty,
        variance_qty,
        transaction_id,
    })
}

async fn last_insert_id<C: ConnectionTrait>(db: &C) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Could not read the new row id.".to_string()]))?;
    Ok(row.try_get("", "id")?)
}

pub async fn run_reconciliation(
    db: &DatabaseConnection,
    input: RunInventoryReconciliationInput,
) -> AppResult<InventoryReconciliationRun> {
    // The tolerance is the tenant setting (Settings > Inventory).
    let break_threshold = crate::settings::catalog::decimal_setting(db, SETTING_RECONCILIATION_DRIFT_BREAK).await?;
    let tx = db.begin().await?;
    let run_code = next_doc("RECON");
    let today = Utc::now().format("%Y-%m-%d").to_string();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_reconciliation_runs
            (run_code, run_date, status, checked_by_id, started_at)
         VALUES (?, ?, 'completed', ?, ?)",
        [
            run_code.clone().into(),
            today.into(),
            input.actor_id.map_or(Value::BigInt(None), Value::from),
            now_iso().into(),
        ],
    ))
    .await?;
    let run_id_row = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to create reconciliation run.".to_string()]))?;
    let run_id: i64 = run_id_row.try_get("", "id")?;

    // The expected on-hand is the sum of the movements' on-hand effect, from the one table the
    // balance itself follows (`ledger::ON_HAND_MOVEMENTS`), so the two can never drift apart.
    let ledger_sql = format!(
        "WITH ledger AS (
            SELECT t.article_id, t.warehouse_id, t.location_id,
                   SUM({}) AS ledger_expected_on_hand
            FROM inventory_transactions t
            GROUP BY t.article_id, t.warehouse_id, t.location_id
         )
         SELECT sb.article_id, sb.warehouse_id, sb.location_id,
                CAST(sb.on_hand_qty AS REAL) AS balance_on_hand,
                CAST(COALESCE(l.ledger_expected_on_hand, 0) AS REAL) AS ledger_expected_on_hand
         FROM stock_balances sb
         LEFT JOIN ledger l
           ON l.article_id = sb.article_id
          AND l.warehouse_id = sb.warehouse_id
          AND l.location_id = sb.location_id",
        crate::inventory::ledger::on_hand_delta_sql("t")
    );
    let rows = tx
        .query_all(Statement::from_string(DbBackend::Sqlite, ledger_sql))
        .await?;

    let mut checked_rows = 0_i64;
    let mut drift_rows = 0_i64;
    for row in rows {
        checked_rows += 1;
        let balance_on_hand: f64 = row.try_get("", "balance_on_hand")?;
        let ledger_expected_on_hand: f64 = row.try_get("", "ledger_expected_on_hand")?;
        let drift_qty = balance_on_hand - ledger_expected_on_hand;
        if drift_qty.abs() <= f64::EPSILON {
            continue;
        }
        drift_rows += 1;
        let is_break = i64::from(drift_qty.abs() >= break_threshold);
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_reconciliation_findings
                (run_id, article_id, warehouse_id, location_id, balance_on_hand, ledger_expected_on_hand, drift_qty, is_break, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                run_id.into(),
                row.try_get::<i64>("", "article_id")?.into(),
                row.try_get::<i64>("", "warehouse_id")?.into(),
                row.try_get::<i64>("", "location_id")?.into(),
                balance_on_hand.into(),
                ledger_expected_on_hand.into(),
                drift_qty.into(),
                is_break.into(),
                now_iso().into(),
            ],
        ))
        .await?;
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_reconciliation_runs
         SET checked_rows = ?, drift_rows = ?, finished_at = ?
         WHERE id = ?",
        [checked_rows.into(), drift_rows.into(), now_iso().into(), run_id.into()],
    ))
    .await?;
    tx.commit().await?;
    get_reconciliation_run(db, run_id).await
}

pub async fn list_count_sessions(db: &DatabaseConnection) -> AppResult<Vec<InventoryCountSession>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, session_code, warehouse_id, location_id, status, critical_abs_threshold,
                    submitted_by_id, submitted_at, posted_by_id, posted_at, reversed_by_id, reversed_at,
                    reversal_reason, row_version, created_at, updated_at
             FROM inventory_count_sessions
             ORDER BY id DESC"
                .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_count_session).collect()
}

pub async fn list_count_lines(db: &DatabaseConnection, session_id: i64) -> AppResult<Vec<InventoryCountLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.id, cl.session_id, cl.article_id, a.article_code, a.article_name, cl.warehouse_id, cl.location_id,
                    sl.code AS location_code, cl.system_qty, cl.counted_qty, cl.variance_qty, cl.variance_reason_code,
                    cl.is_critical, cl.approval_required, cl.approved_by_id,
                    COALESCE(ua.display_name, ua.username) AS approved_by_name, cl.approved_at, cl.approval_note,
                    cl.posted_transaction_id, cl.reversed_transaction_id, cl.row_version, cl.created_at, cl.updated_at
             FROM inventory_count_lines cl
             JOIN articles a ON a.id = cl.article_id
             JOIN stock_locations sl ON sl.id = cl.location_id
             LEFT JOIN user_accounts ua ON ua.id = cl.approved_by_id
             WHERE cl.session_id = ?
             ORDER BY cl.id ASC",
            [session_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_count_line).collect()
}

pub async fn list_reconciliation_runs(db: &DatabaseConnection) -> AppResult<Vec<InventoryReconciliationRun>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, run_code, run_date, status, checked_rows, drift_rows, checked_by_id, started_at, finished_at
             FROM inventory_reconciliation_runs
             ORDER BY id DESC"
                .to_string(),
        ))
        .await?;
    rows.into_iter().map(map_recon_run).collect()
}

pub async fn list_reconciliation_findings(
    db: &DatabaseConnection,
    run_id: i64,
) -> AppResult<Vec<InventoryReconciliationFinding>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT f.id, f.run_id, f.article_id, a.article_code, a.article_name,
                    f.warehouse_id, w.code AS warehouse_code, f.location_id, sl.code AS location_code,
                    f.balance_on_hand, f.ledger_expected_on_hand, f.drift_qty, f.is_break, f.created_at
             FROM inventory_reconciliation_findings f
             JOIN articles a ON a.id = f.article_id
             JOIN warehouses w ON w.id = f.warehouse_id
             JOIN stock_locations sl ON sl.id = f.location_id
             WHERE f.run_id = ?
             ORDER BY ABS(f.drift_qty) DESC, f.id ASC",
            [run_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_recon_finding).collect()
}

async fn get_count_line(db: &DatabaseConnection, line_id: i64) -> AppResult<InventoryCountLine> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.id, cl.session_id, cl.article_id, a.article_code, a.article_name, cl.warehouse_id, cl.location_id,
                    sl.code AS location_code, cl.system_qty, cl.counted_qty, cl.variance_qty, cl.variance_reason_code,
                    cl.is_critical, cl.approval_required, cl.approved_by_id,
                    COALESCE(ua.display_name, ua.username) AS approved_by_name, cl.approved_at, cl.approval_note,
                    cl.posted_transaction_id, cl.reversed_transaction_id, cl.row_version, cl.created_at, cl.updated_at
             FROM inventory_count_lines cl
             JOIN articles a ON a.id = cl.article_id
             JOIN stock_locations sl ON sl.id = cl.location_id
             LEFT JOIN user_accounts ua ON ua.id = cl.approved_by_id
             WHERE cl.id = ?",
            [line_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_count_lines".to_string(),
            id: line_id.to_string(),
        })?;
    map_count_line(row)
}

async fn get_reconciliation_run(db: &DatabaseConnection, run_id: i64) -> AppResult<InventoryReconciliationRun> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, run_code, run_date, status, checked_rows, drift_rows, checked_by_id, started_at, finished_at
             FROM inventory_reconciliation_runs
             WHERE id = ?",
            [run_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_reconciliation_runs".to_string(),
            id: run_id.to_string(),
        })?;
    map_recon_run(row)
}

fn map_count_session(row: sea_orm::QueryResult) -> AppResult<InventoryCountSession> {
    Ok(InventoryCountSession {
        id: row.try_get("", "id")?,
        session_code: row.try_get("", "session_code")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        location_id: row.try_get("", "location_id")?,
        status: row.try_get("", "status")?,
        critical_abs_threshold: row.try_get("", "critical_abs_threshold")?,
        submitted_by_id: row.try_get("", "submitted_by_id")?,
        submitted_at: row.try_get("", "submitted_at")?,
        posted_by_id: row.try_get("", "posted_by_id")?,
        posted_at: row.try_get("", "posted_at")?,
        reversed_by_id: row.try_get("", "reversed_by_id")?,
        reversed_at: row.try_get("", "reversed_at")?,
        reversal_reason: row.try_get("", "reversal_reason")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_count_line(row: sea_orm::QueryResult) -> AppResult<InventoryCountLine> {
    Ok(InventoryCountLine {
        id: row.try_get("", "id")?,
        session_id: row.try_get("", "session_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        system_qty: row.try_get("", "system_qty")?,
        counted_qty: row.try_get("", "counted_qty")?,
        variance_qty: row.try_get("", "variance_qty")?,
        variance_reason_code: row.try_get("", "variance_reason_code")?,
        is_critical: row.try_get("", "is_critical")?,
        approval_required: row.try_get("", "approval_required")?,
        approved_by_id: row.try_get("", "approved_by_id")?,
        approved_by_name: row.try_get("", "approved_by_name").ok().flatten(),
        approved_at: row.try_get("", "approved_at")?,
        approval_note: row.try_get("", "approval_note")?,
        posted_transaction_id: row.try_get("", "posted_transaction_id")?,
        reversed_transaction_id: row.try_get("", "reversed_transaction_id")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_recon_run(row: sea_orm::QueryResult) -> AppResult<InventoryReconciliationRun> {
    Ok(InventoryReconciliationRun {
        id: row.try_get("", "id")?,
        run_code: row.try_get("", "run_code")?,
        run_date: row.try_get("", "run_date")?,
        status: row.try_get("", "status")?,
        checked_rows: row.try_get("", "checked_rows")?,
        drift_rows: row.try_get("", "drift_rows")?,
        checked_by_id: row.try_get("", "checked_by_id")?,
        started_at: row.try_get("", "started_at")?,
        finished_at: row.try_get("", "finished_at")?,
    })
}

fn map_recon_finding(row: sea_orm::QueryResult) -> AppResult<InventoryReconciliationFinding> {
    Ok(InventoryReconciliationFinding {
        id: row.try_get("", "id")?,
        run_id: row.try_get("", "run_id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        warehouse_code: row.try_get("", "warehouse_code")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        balance_on_hand: row.try_get("", "balance_on_hand")?,
        ledger_expected_on_hand: row.try_get("", "ledger_expected_on_hand")?,
        drift_qty: row.try_get("", "drift_qty")?,
        is_break: row.try_get("", "is_break")?,
        created_at: row.try_get("", "created_at")?,
    })
}

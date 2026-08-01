use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    ApproveInventoryCountLineInput, CreateInventoryCountSessionInput, InventoryCountLine, InventoryCountSession,
    InventoryReconciliationFinding, InventoryReconciliationRun, PostInventoryCountSessionInput, ReverseInventoryCountSessionInput,
    RunInventoryReconciliationInput, TransitionInventoryCountSessionInput, UpsertInventoryCountLineInput,
};

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

async fn ensure_lookup_variance_reason_active(db: &DatabaseConnection, code: &str) -> AppResult<()> {
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
            .ok_or_else(|| AppError::ValidationFailed(vec!["location_id does not exist.".to_string()]))?;
        Ok((wh_row.try_get("", "warehouse_id")?, 0.0))
    }
}

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
            available_qty = excluded.available_qty,
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
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions
            (article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason, performed_by_id, performed_at)
         VALUES (?, ?, ?, NULL, ?, ?, 'COUNT_SESSION', ?, ?, ?, ?, ?)",
        [
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
    if input.critical_abs_threshold.unwrap_or(5.0) < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "critical_abs_threshold must be >= 0.".to_string(),
        ]));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_sessions
            (session_code, warehouse_id, location_id, status, critical_abs_threshold, opened_by_id, created_at, updated_at)
         VALUES (?, ?, ?, 'draft', ?, ?, ?, ?)",
        [
            next_doc("CC").into(),
            input.warehouse_id.into(),
            input.location_id.map_or(Value::BigInt(None), Value::from),
            input.critical_abs_threshold.unwrap_or(5.0).into(),
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
            "count session row_version mismatch.".to_string(),
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

pub async fn upsert_count_line(db: &DatabaseConnection, input: UpsertInventoryCountLineInput) -> AppResult<InventoryCountLine> {
    if input.counted_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "counted_qty must be >= 0.".to_string(),
        ]));
    }
    let session = ensure_count_session(db, input.session_id).await?;
    if session.status != "draft" && session.status != "counting" {
        return Err(AppError::ValidationFailed(vec![
            "Count lines can only be edited in draft/counting status.".to_string(),
        ]));
    }
    let (warehouse_id, system_qty) = get_balance(db, input.article_id, input.location_id).await?;
    if warehouse_id != session.warehouse_id {
        return Err(AppError::ValidationFailed(vec![
            "Count line location must belong to session warehouse.".to_string(),
        ]));
    }
    let variance_qty = input.counted_qty - system_qty;
    if variance_qty.abs() > f64::EPSILON && input.variance_reason_code.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "variance_reason_code is mandatory when variance exists.".to_string(),
        ]));
    }
    if let Some(code) = input.variance_reason_code.as_deref() {
        ensure_lookup_variance_reason_active(db, code).await?;
    }
    let is_critical = i64::from(variance_qty.abs() >= session.critical_abs_threshold);
    let approval_required = i64::from(variance_qty.abs() > f64::EPSILON);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_lines
            (session_id, article_id, warehouse_id, location_id, system_qty, counted_qty, variance_qty,
             variance_reason_code, is_critical, approval_required, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.id
             FROM inventory_count_lines cl
             WHERE cl.session_id = ? AND cl.article_id = ? AND cl.location_id = ?",
            [input.session_id.into(), input.article_id.into(), input.location_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Unable to upsert count line.".to_string()]))?;
    get_count_line(db, row.try_get("", "id")?).await
}

pub async fn approve_count_line(db: &DatabaseConnection, input: ApproveInventoryCountLineInput) -> AppResult<InventoryCountLine> {
    if input.reviewer_evidence.trim().len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "reviewer_evidence must be explicit and at least 8 characters.".to_string(),
        ]));
    }
    let line = get_count_line(db, input.line_id).await?;
    if line.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "count line row_version mismatch.".to_string(),
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

pub async fn post_count_session(db: &DatabaseConnection, input: PostInventoryCountSessionInput) -> AppResult<InventoryCountSession> {
    let session = ensure_count_session(db, input.session_id).await?;
    if session.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "count session row_version mismatch.".to_string(),
        ]));
    }
    if session.status != "approved" {
        return Err(AppError::ValidationFailed(vec![
            "Count session must be approved before posting.".to_string(),
        ]));
    }
    let lines = list_count_lines(db, input.session_id).await?;
    if lines.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Count session has no lines.".to_string(),
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
                    "Line {} cannot post without reviewer evidence.",
                    line.id
                )]));
            }
        }
    }

    let tx = db.begin().await?;
    for line in lines {
        if line.variance_qty.abs() <= f64::EPSILON {
            continue;
        }
        let (_, current_qty) = get_balance(&tx, line.article_id, line.location_id).await?;
        let next_qty = current_qty + line.variance_qty;
        if next_qty < 0.0 {
            return Err(AppError::ValidationFailed(vec![format!(
                "Posting would produce negative stock for line {}.",
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
    if input.reason.trim().len() < 5 {
        return Err(AppError::ValidationFailed(vec![
            "Reversal reason must be explicit.".to_string(),
        ]));
    }
    let session = ensure_count_session(db, input.session_id).await?;
    if session.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "count session row_version mismatch.".to_string(),
        ]));
    }
    if session.status != "posted" {
        return Err(AppError::ValidationFailed(vec![
            "Only posted count sessions can be reversed.".to_string(),
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
        if next_qty < 0.0 {
            return Err(AppError::ValidationFailed(vec![format!(
                "Reversal would produce negative stock for line {}.",
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

pub async fn run_reconciliation(
    db: &DatabaseConnection,
    input: RunInventoryReconciliationInput,
) -> AppResult<InventoryReconciliationRun> {
    let break_threshold = input.drift_break_threshold.unwrap_or(0.0001_f64).max(0.0);
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

    let rows = tx
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "WITH ledger AS (
                SELECT article_id, warehouse_id, location_id,
                       SUM(
                         CASE movement_type
                           WHEN 'ADJUST' THEN quantity
                           WHEN 'ISSUE' THEN -quantity
                           WHEN 'RETURN' THEN quantity
                           WHEN 'TRANSFER_OUT' THEN -quantity
                           WHEN 'TRANSFER_IN' THEN quantity
                           WHEN 'GR_ACCEPT' THEN quantity
                           WHEN 'REPAIRABLE_RELEASE' THEN -quantity
                           WHEN 'REPAIRABLE_RETURN' THEN quantity
                           WHEN 'COUNT_ADJUST' THEN quantity
                           WHEN 'COUNT_REVERSAL' THEN quantity
                           ELSE 0
                         END
                       ) AS ledger_expected_on_hand
                FROM inventory_transactions
                GROUP BY article_id, warehouse_id, location_id
             )
             SELECT sb.article_id, sb.warehouse_id, sb.location_id,
                    CAST(sb.on_hand_qty AS REAL) AS balance_on_hand,
                    CAST(COALESCE(l.ledger_expected_on_hand, 0) AS REAL) AS ledger_expected_on_hand
             FROM stock_balances sb
             LEFT JOIN ledger l
               ON l.article_id = sb.article_id
              AND l.warehouse_id = sb.warehouse_id
              AND l.location_id = sb.location_id"
                .to_string(),
        ))
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
                    cl.is_critical, cl.approval_required, cl.approved_by_id, cl.approved_at, cl.approval_note,
                    cl.posted_transaction_id, cl.reversed_transaction_id, cl.row_version, cl.created_at, cl.updated_at
             FROM inventory_count_lines cl
             JOIN articles a ON a.id = cl.article_id
             JOIN stock_locations sl ON sl.id = cl.location_id
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
                    cl.is_critical, cl.approval_required, cl.approved_by_id, cl.approved_at, cl.approval_note,
                    cl.posted_transaction_id, cl.reversed_transaction_id, cl.row_version, cl.created_at, cl.updated_at
             FROM inventory_count_lines cl
             JOIN articles a ON a.id = cl.article_id
             JOIN stock_locations sl ON sl.id = cl.location_id
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

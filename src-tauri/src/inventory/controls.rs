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

/// Refusal code: the count of this session, article and location already moved the stock on this
/// PC (the same session posted or reversed on another PC first); also the code of a push of that
/// movement closed because the server holds the same count (`inventory::count_sync`).
pub const COUNT_ALREADY_POSTED: &str = "COUNT_ALREADY_POSTED";

/// Refusal code: a count session is posted or reversed only by its posting or its reversal (the
/// stock moves with them), never by a status change.
pub const REFUSED_COUNT_STATUS_BY_POSTING_ONLY: &str = "COUNT_SESSION_STATUS_POSTING_ONLY";

/// Refusal code: a count session is approved only once every line whose variance needs a
/// reviewer's approval has it.
pub const REFUSED_COUNT_LINE_NOT_APPROVED: &str = "COUNT_LINE_NOT_APPROVED";

/// Tenant setting (catalog, decimal): a ledger / balance gap of at least this much is a
/// reconciliation break.
pub const SETTING_RECONCILIATION_DRIFT_BREAK: &str = "inventory.reconciliation_drift_break";

fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn next_doc(prefix: &str) -> String {
    format!("{prefix}-{}", Utc::now().timestamp_millis())
}

async fn ensure_count_session<C: ConnectionTrait>(db: &C, session_id: i64) -> AppResult<InventoryCountSession> {
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
            "GATE_FIN_REFERENCE_VALUE_INACTIVE:code={code}: variance_reason_code '{code}' is not active."
        )]));
    }
    Ok(())
}

pub(crate) async fn get_balance<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<(i64, f64)> {
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

/// Kind of the seeded sync id of each count movement type.
pub(crate) fn count_movement_kind(movement_type: &str) -> Option<&'static str> {
    match movement_type {
        "COUNT_ADJUST" => Some("count_adjust"),
        "COUNT_REVERSAL" => Some("count_reversal"),
        _ => None,
    }
}

/// Sync id of the count movement of a session, article and location: the same on every PC
/// (`{session}:{article}:{location}` sync ids, all three immutable), so a session posted (or
/// reversed) on two PCs gives one movement, received once and applied once (AU-100).
///
/// Every row has its sync id from its insert on (capture trigger, installed at every startup before
/// any command). A database without capture (tests, tools) may hold rows without one; such a
/// session was never synchronised, so no other PC can post it, and its movement gets a random id
/// (`sync::identity::user_keyed_entity_sync_id` does the same).
async fn count_movement_sync_id<C: ConnectionTrait>(
    db: &C,
    movement_type: &str,
    session_id: i64,
    article_id: i64,
    location_id: i64,
) -> AppResult<String> {
    let kind = count_movement_kind(movement_type)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("{movement_type} is not a count movement")))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT (SELECT entity_sync_id FROM inventory_count_sessions WHERE id = ?) AS session_sync_id,
                    (SELECT entity_sync_id FROM articles WHERE id = ?) AS article_sync_id,
                    (SELECT entity_sync_id FROM stock_locations WHERE id = ?) AS location_sync_id",
            [session_id.into(), article_id.into(), location_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count movement identity query returned no row")))?;
    let id = |column: &str| -> AppResult<Option<String>> {
        Ok(row
            .try_get::<Option<String>>("", column)?
            .filter(|v| !v.trim().is_empty()))
    };
    Ok(
        match (id("session_sync_id")?, id("article_sync_id")?, id("location_sync_id")?) {
            (Some(session), Some(article), Some(location)) => {
                crate::sync::identity::seeded_entity_sync_id(kind, &format!("{session}:{article}:{location}"))
            }
            _ => crate::sync::identity::new_entity_sync_id(),
        },
    )
}

/// Sync id of the audit link of a count movement: one link per movement on every PC.
pub(crate) fn count_audit_link_sync_id(movement_sync_id: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("count_audit_link", movement_sync_id)
}

/// Writes a count movement (`COUNT_ADJUST` or `COUNT_REVERSAL`) with its seeded sync id
/// ([`count_movement_sync_id`]). Refused ([`COUNT_ALREADY_POSTED`]) when this PC already holds the
/// movement of that count — received from a PC where the same session was posted or reversed first,
/// whatever its id — so the stock never moves twice for one count. Returns `(local id, sync id)`.
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
) -> AppResult<(i64, String)> {
    // Count postings and reversals are dated today; refused when today falls in a closed period.
    crate::inventory::periods::ensure_posting_date_open(db, &crate::inventory::fx::today()).await?;
    let already = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS held FROM inventory_transactions
             WHERE movement_type = ? AND source_type = 'COUNT_SESSION' AND source_id = ?
               AND article_id = ? AND location_id = ?
             LIMIT 1",
            [
                movement_type.into(),
                session_id.into(),
                article_id.into(),
                location_id.into(),
            ],
        ))
        .await?;
    if already.is_some() {
        return Err(crate::errors::org_validation_failed(
            COUNT_ALREADY_POSTED,
            format!(
                "The count {source_ref} already moved the stock of this article at this location on this PC (posted or reversed on another PC): refresh the session."
            ),
            crate::errors::issue_params(&[("session", source_ref.to_string())]),
        ));
    }
    let entity_sync_id = count_movement_sync_id(db, movement_type, session_id, article_id, location_id).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions
            (entity_sync_id, article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason, performed_by_id, performed_at, row_version)
         VALUES (?, ?, ?, ?, NULL, ?, ?, 'COUNT_SESSION', ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.clone().into(),
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
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_INV_SAVE_FAILED: Could not read transaction id.".to_string()])
        })?;
    Ok((tx_row.try_get("", "id")?, entity_sync_id))
}

pub async fn create_count_session(
    db: &DatabaseConnection,
    input: CreateInventoryCountSessionInput,
) -> AppResult<InventoryCountSession> {
    // A variance is judged against the plant's four-eyes tolerance when it is entered
    // (`count_four_eyes`); the session records no tolerance of its own (migration 259).
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_sessions
            (session_code, warehouse_id, location_id, status, critical_abs_threshold, opened_by_id, created_at, updated_at)
         VALUES (?, ?, ?, 'draft', NULL, ?, ?, ?)",
        [
            next_doc("CC").into(),
            input.warehouse_id.into(),
            input.location_id.map_or(Value::BigInt(None), Value::from),
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
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_INV_SAVE_FAILED: Session creation failed.".to_string()])
        })?;
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
    // Posted and reversed move the stock: only `post_count_session` and `reverse_count_session`
    // (inv.manage for a reversal) reach them, never a status change (AU-70).
    if input.next_status == "posted" || input.next_status == "reversed" {
        return Err(crate::errors::org_validation_failed(
            REFUSED_COUNT_STATUS_BY_POSTING_ONLY,
            format!(
                "A count session is {} only by its {} (the stock moves with it), not by a status change.",
                input.next_status,
                if input.next_status == "posted" {
                    "posting"
                } else {
                    "reversal"
                }
            ),
            crate::errors::issue_params(&[("status", input.next_status.clone())]),
        ));
    }
    let allowed = match current.status.as_str() {
        "draft" => ["counting", "cancelled"].as_slice(),
        "counting" => ["submitted", "cancelled"].as_slice(),
        "submitted" => ["approved", "cancelled"].as_slice(),
        "approved" => ["cancelled"].as_slice(),
        _ => [].as_slice(),
    };
    if !allowed.iter().any(|x| *x == input.next_status) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INV_COUNT_TRANSITION:from={from},to={to}: Invalid count-session transition: {from} -> {to}",
            from = current.status,
            to = input.next_status
        )]));
    }
    if input.next_status == "approved" {
        // A variance is approved line by line by a reviewer (with evidence) before the count is.
        let pending = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS pending FROM inventory_count_lines
                 WHERE session_id = ? AND approval_required = 1
                   AND (approved_by_id IS NULL OR trim(COALESCE(approval_note, '')) = '')",
                [input.session_id.into()],
            ))
            .await?
            .map(|row| row.try_get::<i64>("", "pending"))
            .transpose()?
            .unwrap_or(0);
        if pending > 0 {
            return Err(crate::errors::org_validation_failed(
                REFUSED_COUNT_LINE_NOT_APPROVED,
                format!(
                    "{pending} counted line(s) with a variance still need a reviewer's approval before the count is approved."
                ),
                crate::errors::issue_params(&[("lines", pending.to_string())]),
            ));
        }
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

/// The audit target of a count session: its sync id (the same on every PC, as
/// `inventory::count_sync` names it); a session of a database without capture has none and is
/// named by its local id.
async fn session_audit_target<C: ConnectionTrait>(db: &C, session_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(NULLIF(TRIM(entity_sync_id), ''), CAST(id AS TEXT)) AS target
             FROM inventory_count_sessions WHERE id = ?",
            [session_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_count_sessions".to_string(),
            id: session_id.to_string(),
        })?;
    Ok(row.try_get("", "target")?)
}

/// Refusal of a count line entered or approved against a version that is no longer the one on
/// record (another entry was made in between).
fn count_line_changed_elsewhere() -> AppError {
    AppError::ValidationFailed(vec![
        "GATE_RECORD_CHANGED_ELSEWHERE: count line row_version mismatch.".to_string()
    ])
}

pub async fn upsert_count_line(
    db: &DatabaseConnection,
    input: UpsertInventoryCountLineInput,
) -> AppResult<InventoryCountLine> {
    if input.counted_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNTED_NEGATIVE: counted_qty must be >= 0.".to_string(),
        ]));
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
            "GATE_INV_COUNT_LOCATION_WAREHOUSE: Count line location must belong to session warehouse.".to_string(),
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
    // Above the four-eyes tolerance ("critique"), judged now, with the stock the variance is set
    // against; the thresholds used go to the line's audit.
    let verdict =
        crate::inventory::count_four_eyes::evaluate(&tx, input.article_id, input.location_id, variance_qty, system_qty)
            .await?;
    let is_critical = i64::from(verdict.above_tolerance);
    // The unit cost it was judged with and the variance's value, kept on the line for the PCs that
    // receive it (CMMS Advisor Q10, `count_four_eyes::judged_unit_cost`).
    let (entry_unit_cost, entry_variance_value) = verdict.entry_values(variance_qty);
    let approval_required = i64::from(variance_qty.abs() > f64::EPSILON);
    // The count's mark is taken now, with the stock on record its variance is set against: the
    // counts this PC knows at this moment, not at posting, so a count received in between still
    // shows as a double count (`inventory::negative_stock`, CMMS Advisor). Each entry refreshes it.
    let (replica_id, follows_json) =
        crate::inventory::negative_stock::count_mark(&tx, input.article_id, input.location_id).await?;
    // A line is entered again only against the version its author saw (AU-68): read in this
    // transaction, and the update itself is guarded on it, so an entry made in between (another
    // screen, another PC received by sync) is never overwritten silently.
    let existing_version: Option<i64> = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM inventory_count_lines
             WHERE session_id = ? AND article_id = ? AND location_id = ?",
            [
                input.session_id.into(),
                input.article_id.into(),
                input.location_id.into(),
            ],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "row_version"))
        .transpose()?;
    if existing_version != input.expected_row_version {
        return Err(count_line_changed_elsewhere());
    }
    let now = now_iso();
    let written = if let Some(expected) = existing_version {
        // Entered again: the approval given to the previous quantity no longer holds.
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines SET
                 counted_qty = ?, system_qty = ?, variance_qty = ?, variance_reason_code = ?,
                 is_critical = ?, approval_required = ?, counted_by_id = ?,
                 approved_by_id = NULL, approved_at = NULL, approval_note = NULL, self_approved = 0,
                 entry_unit_cost = ?, entry_variance_value = ?,
                 count_replica_id = ?, count_follows_json = ?, count_path = ?,
                 row_version = row_version + 1, updated_at = ?
             WHERE session_id = ? AND article_id = ? AND location_id = ? AND row_version = ?",
            [
                input.counted_qty.into(),
                system_qty.into(),
                variance_qty.into(),
                input
                    .variance_reason_code
                    .clone()
                    .map_or(Value::String(None), Value::from),
                is_critical.into(),
                approval_required.into(),
                input.actor_id.map_or(Value::BigInt(None), Value::from),
                entry_unit_cost.into(),
                entry_variance_value.into(),
                replica_id.into(),
                follows_json.into(),
                crate::inventory::negative_stock::COUNT_PATH_COUNT_SESSION.into(),
                now.clone().into(),
                input.session_id.into(),
                input.article_id.into(),
                input.location_id.into(),
                expected.into(),
            ],
        ))
        .await?
    } else {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inventory_count_lines
                (session_id, article_id, warehouse_id, location_id, system_qty, counted_qty, variance_qty,
                 variance_reason_code, is_critical, approval_required, counted_by_id, created_at, updated_at,
                 count_replica_id, count_follows_json, count_path, entry_unit_cost, entry_variance_value)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_id, article_id, location_id) DO NOTHING",
            [
                input.session_id.into(),
                input.article_id.into(),
                warehouse_id.into(),
                input.location_id.into(),
                system_qty.into(),
                input.counted_qty.into(),
                variance_qty.into(),
                input
                    .variance_reason_code
                    .clone()
                    .map_or(Value::String(None), Value::from),
                is_critical.into(),
                approval_required.into(),
                input.actor_id.map_or(Value::BigInt(None), Value::from),
                now.clone().into(),
                now.clone().into(),
                replica_id.into(),
                follows_json.into(),
                crate::inventory::negative_stock::COUNT_PATH_COUNT_SESSION.into(),
                entry_unit_cost.into(),
                entry_variance_value.into(),
            ],
        ))
        .await?
    };
    if written.rows_affected() != 1 {
        return Err(count_line_changed_elsewhere());
    }
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.count.line_entered".to_string(),
            target_type: Some("inventory_count_sessions".to_string()),
            target_id: Some(session_audit_target(&tx, session.id).await?),
            actor_id: input.actor_id,
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Counted quantity entered",
                "count_session_code": session.session_code,
                "article_id": input.article_id,
                "location_id": input.location_id,
                "system_qty": system_qty,
                "counted_qty": input.counted_qty,
                "variance_qty": variance_qty,
                "four_eyes_tolerance": verdict.audit_json(),
            })),
        },
    )
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
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_INV_SAVE_FAILED: Unable to upsert count line.".to_string()])
        })?;
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
    // The line the four-eyes check judges (its flag, its counter, its version) is read in the
    // approval's transaction: a sync apply in between cannot change it under the check.
    let tx = db.begin().await?;
    let line = get_count_line(&tx, input.line_id).await?;
    if line.row_version != input.expected_row_version {
        return Err(count_line_changed_elsewhere());
    }
    if line.approval_required == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_NO_APPROVAL_NEEDED: Approval is not required for zero variance lines.".to_string(),
        ]));
    }
    refuse_reconciliation_list_count(&tx, line.session_id).await?;
    // A line is approved once, while its session is still being counted or reviewed: the approver
    // of a line already approved, or of a posted, reversed or cancelled count, is never rewritten.
    let session = ensure_count_session(&tx, line.session_id).await?;
    if !["draft", "counting", "submitted"].contains(&session.status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_COUNT_LINE_APPROVAL_CLOSED}:status={0}: the count session is {0}: its lines are no longer \
             approved.",
            session.status
        )]));
    }
    if line.approved_by_id.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_COUNT_LINE_ALREADY_APPROVED}: this line is already approved; enter the counted quantity again \
             to have it approved anew."
        )]));
    }
    // Above the four-eyes tolerance, someone other than the counter approves, unless the site
    // opted out (then it is an "auto-approbation", flagged).
    let self_approved = crate::inventory::count_four_eyes::check_approval(
        &tx,
        true,
        line.is_critical == 1,
        line.counted_by_id,
        input.reviewer_id,
    )
    .await?;
    // The tolerance in force at the approval, kept with it (the line's flag was judged at entry).
    let tolerance = crate::inventory::count_four_eyes::evaluate(
        &tx,
        line.article_id,
        line.location_id,
        line.variance_qty,
        line.system_qty,
    )
    .await?;
    let written = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines
             SET approved_by_id = ?, approved_at = ?, approval_note = ?, self_approved = ?,
                 row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input.reviewer_id.into(),
                now_iso().into(),
                input.reviewer_evidence.trim().into(),
                i64::from(self_approved).into(),
                now_iso().into(),
                input.line_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if written.rows_affected() != 1 {
        return Err(count_line_changed_elsewhere());
    }
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.count.line_approved".to_string(),
            target_type: Some("inventory_count_sessions".to_string()),
            target_id: Some(session_audit_target(&tx, line.session_id).await?),
            actor_id: Some(input.reviewer_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": if self_approved { "Count variance self-approved (site opt-out)" } else { "Count variance approved" },
                "count_line_id": line.id,
                "article_id": line.article_id,
                "location_id": line.location_id,
                "variance_qty": line.variance_qty,
                "above_tolerance": line.is_critical == 1,
                "counted_by_id": line.counted_by_id,
                "self_approved": self_approved,
                "note": input.reviewer_evidence.trim(),
                "four_eyes_tolerance": approval_tolerance_json(line.is_critical == 1, &tolerance),
            })),
        },
    )
    .await?;
    tx.commit().await?;
    get_count_line(db, input.line_id).await
}

/// Refusal: a line of a count session that is posted, reversed, approved or cancelled is not
/// approved (its approver is never rewritten).
pub const REFUSED_COUNT_LINE_APPROVAL_CLOSED: &str = "GATE_INV_COUNT_APPROVAL_CLOSED";
/// Refusal: the line is already approved (entered again, it is approved anew).
pub const REFUSED_COUNT_LINE_ALREADY_APPROVED: &str = "GATE_INV_COUNT_ALREADY_APPROVED";

/// Refusal: the line has no self-approval to approve a second time (not posted, reversed, without
/// variance, approved by someone other than its counter, or approved a second time already).
pub const REFUSED_SECOND_APPROVAL_NOT_NEEDED: &str = "GATE_INV_COUNT_SECOND_APPROVAL_NOT_NEEDED";
/// Refusal: the second approval is given by the counter (who also approved it).
pub const REFUSED_SECOND_APPROVAL_SELF: &str = "GATE_INV_COUNT_SECOND_APPROVAL_SELF";

/// The second approval of a posted count line whose variance was approved by its counter: given by
/// someone other than the counter, with a note of at least 8 characters, where the count is applied;
/// audited and synchronised. Above the four-eyes tolerance ("auto-approbation", under the opt-out of
/// the site) it is needed; within it, it is optional (CMMS Advisor Q15): a PC with a stricter
/// tolerance or another cost judges the line above it there. A PC where four-eyes applies holds such
/// a count back (line and movement, `count_four_eyes::inbound_count_movement_hold`) until this
/// version arrives (CMMS Advisor Q8).
pub async fn second_approve_count_line(
    db: &DatabaseConnection,
    input: crate::inventory::domain::SecondApproveInventoryCountLineInput,
) -> AppResult<InventoryCountLine> {
    // The count-approval right (`inv.manage`, plant-wide: stores have no site) of the second
    // approver, checked here too (the command checks it for the signed-in user): this is the right a
    // PC holding the count back checks when the second approval arrives.
    let holds_right = match i32::try_from(input.reviewer_id) {
        Ok(user_id) => {
            crate::auth::rbac::check_permission(
                db,
                user_id,
                crate::rbac::permissions::INV_MANAGE,
                &crate::auth::rbac::PermissionScope::Global,
            )
            .await?
        }
        Err(_) => false,
    };
    if !holds_right {
        return Err(AppError::PermissionDenied(format!(
            "Permission requise : {}",
            crate::rbac::permissions::INV_MANAGE
        )));
    }
    let note = input.reviewer_evidence.trim();
    if note.chars().count() < crate::inventory::negative_stock::REVIEW_REASON_MIN_CHARS {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNT_EVIDENCE: reviewer_evidence must be explicit and at least 8 characters.".to_string(),
        ]));
    }
    let tx = db.begin().await?;
    let line = get_count_line(&tx, input.line_id).await?;
    if line.row_version != input.expected_row_version {
        return Err(count_line_changed_elsewhere());
    }
    // Any variance approved by its counter (CMMS Advisor Q15), above the tolerance here or not.
    let self_approved = line.variance_qty.abs() > f64::EPSILON
        && line.approved_by_id.is_some()
        && line.approved_by_id == line.counted_by_id
        && line.posted_transaction_id.is_some()
        && line.reversed_transaction_id.is_none()
        && line.second_approved_by_id.is_none();
    if !self_approved {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_SECOND_APPROVAL_NOT_NEEDED}: this count line has no self-approval to approve a second time."
        )]));
    }
    if line.counted_by_id == Some(input.reviewer_id) || line.approved_by_id == Some(input.reviewer_id) {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_SECOND_APPROVAL_SELF}: you counted and approved this variance: another person gives the second \
             approval."
        )]));
    }
    let now = now_iso();
    let written = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines
             SET second_approved_by_id = ?, second_approved_at = ?, second_approval_note = ?,
                 row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND row_version = ? AND second_approved_by_id IS NULL",
            [
                input.reviewer_id.into(),
                now.clone().into(),
                note.into(),
                now.into(),
                input.line_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if written.rows_affected() != 1 {
        return Err(count_line_changed_elsewhere());
    }
    crate::audit::writer::write_audit_event_in(
        &tx,
        crate::audit::writer::AuditEventInput {
            action_code: "inventory.count.line_second_approved".to_string(),
            target_type: Some("inventory_count_sessions".to_string()),
            target_id: Some(session_audit_target(&tx, line.session_id).await?),
            actor_id: Some(input.reviewer_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Self-approved count variance approved a second time by another person",
                "count_line_id": line.id,
                "article_id": line.article_id,
                "location_id": line.location_id,
                "variance_qty": line.variance_qty,
                "above_tolerance": line.is_critical == 1,
                "counted_by_id": line.counted_by_id,
                "approved_by_id": line.approved_by_id,
                "note": note,
            })),
        },
    )
    .await?;
    tx.commit().await?;
    get_count_line(db, input.line_id).await
}

/// The tolerance kept in an approval's audit: the line's flag (judged at entry, as the entry's
/// audit records with the thresholds then) and the thresholds in force at the approval.
pub(crate) fn approval_tolerance_json(
    above_at_entry: bool,
    at_approval: &crate::inventory::count_four_eyes::ToleranceVerdict,
) -> serde_json::Value {
    serde_json::json!({
        "above_tolerance_at_entry": above_at_entry,
        "at_approval": at_approval.audit_json(),
    })
}

/// Refusal of the generic count steps (line approval, posting) on a count recorded from the
/// stock-to-reconcile list: it is approved there, where its posting closes the item in the same
/// step and is refused when the stock moved since the count (D11 Q4a).
pub const REFUSED_RECONCILE_LIST_COUNT: &str = "GATE_INV_COUNT_RECONCILE_LIST";

async fn refuse_reconciliation_list_count<C: ConnectionTrait>(conn: &C, session_id: i64) -> AppResult<()> {
    let listed = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS listed FROM inventory_count_lines WHERE session_id = ? AND count_path = ? LIMIT 1",
            [
                session_id.into(),
                crate::inventory::negative_stock::COUNT_PATH_RECONCILIATION.into(),
            ],
        ))
        .await?
        .is_some();
    if listed {
        return Err(AppError::ValidationFailed(vec![format!(
            "{REFUSED_RECONCILE_LIST_COUNT}: this count was recorded from the stock-to-reconcile list: approve it \
             there (stock to reconcile, count waiting for approval)."
        )]));
    }
    Ok(())
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
    refuse_reconciliation_list_count(db, input.session_id).await?;
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
                    "GATE_INV_COUNT_VARIANCE_REASON: Line {} has variance but no variance reason.",
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
        let (tx_id, tx_sync_id) = insert_count_transaction(
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
                (entity_sync_id, transaction_id, source_entity_type, source_entity_id, reviewer_id, reviewer_evidence, created_at)
             VALUES (?, ?, 'inventory_count_line', ?, ?, ?, ?)",
            [
                count_audit_link_sync_id(&tx_sync_id).into(),
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
            "GATE_INV_REVERSAL_REASON:min={0}: Reversal reason must be explicit (at least {0} characters).",
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
        let Some(posted_transaction_id) = line.posted_transaction_id else {
            continue;
        };
        // What the posting moved, as the ledger holds it: when the same count was posted on two PCs
        // with different lines, the movement kept on every PC can differ from this PC's line until
        // its line conflict is resolved (`inventory::count_sync`); the reversal undoes the movement.
        let posted_qty: f64 = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT quantity FROM inventory_transactions WHERE id = ?",
                [posted_transaction_id.into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::ValidationFailed(vec![format!(
                    "GATE_INV_COUNT_LINE_NO_MOVEMENT:line={id}: Line {id} has no posted movement.",
                    id = line.id
                )])
            })?
            .try_get("", "quantity")?;
        if posted_qty.abs() <= f64::EPSILON {
            continue;
        }
        let (_, current_qty) = get_balance(&tx, line.article_id, line.location_id).await?;
        let next_qty = current_qty - posted_qty;
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
        let (reverse_tx_id, _) = insert_count_transaction(
            &tx,
            line.article_id,
            line.warehouse_id,
            line.location_id,
            -posted_qty,
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

/// What [`record_location_count_in`] (or [`approve_location_count_in`]) wrote.
pub(crate) struct PostedLocationCount {
    pub session_id: i64,
    pub session_code: String,
    pub line_id: i64,
    pub system_qty: f64,
    pub variance_qty: f64,
    /// The count movement (none when the count equals the stock on record, or while it waits for
    /// its approval).
    pub transaction_id: Option<i64>,
    /// Approved by its counter under the site's opt-out ("auto-approbation").
    pub self_approved: bool,
    /// Counted, not posted: it waits for a second person's approval.
    pub pending_approval: bool,
    /// The tolerance applied, as kept in the audit (at the count; at an approval, the line's flag
    /// judged at the count and the thresholds in force at the approval).
    pub tolerance_json: serde_json::Value,
    /// The codes of the waiting counts of the pair this count replaced (their sessions cancelled).
    pub replaced_sessions: Vec<String>,
}

/// The pending list counts of an article and location (counted, waiting for a second person):
/// a new count of the pair from the list replaces them (their sessions are cancelled).
const PENDING_LIST_COUNT_SESSIONS: &str = "SELECT cl.session_id FROM inventory_count_lines cl
     JOIN inventory_count_sessions cs ON cs.id = cl.session_id
     WHERE cl.article_id = ? AND cl.location_id = ? AND cl.count_path = 'reconciliation'
       AND cl.approved_by_id IS NULL AND cs.status = 'submitted'";

/// Records a count of one article at one location as a count session of that location with one
/// line, in the caller's transaction — the rows of the count workflow, so the count reaches the
/// other PCs and shows in the count history like any count. Used where a count is the resolution
/// of a review (`inventory::negative_stock`); the caller checks the right and the reason.
///
/// Within the four-eyes tolerance (or without variance), and above it under the opt-out of the
/// organisation's only site (flagged "auto-approbation"), the count is approved (the evidence is
/// the approval note) and posted by the person counting at once: the `COUNT_ADJUST` movement of
/// the variance and its audit link. Above it elsewhere, the count is saved as counted (session
/// `submitted`, line not approved, no movement): a second person approves and posts it
/// ([`approve_location_count_in`], D11 Q4a). A count of the pair still waiting is replaced.
pub(crate) async fn record_location_count_in<C: ConnectionTrait>(
    tx: &C,
    count: &LocationCount<'_>,
) -> AppResult<PostedLocationCount> {
    if !count.counted_qty.is_finite() || count.counted_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INV_COUNTED_NEGATIVE: counted_qty must be >= 0.".to_string(),
        ]));
    }
    let (warehouse_id, system_qty) = get_balance(tx, count.article_id, count.location_id).await?;
    let variance_qty = count.counted_qty - system_qty;
    let has_variance = variance_qty.abs() > f64::EPSILON;
    if has_variance {
        ensure_lookup_variance_reason_active(tx, count.variance_reason_code).await?;
    }
    let verdict =
        crate::inventory::count_four_eyes::evaluate(tx, count.article_id, count.location_id, variance_qty, system_qty)
            .await?;
    let above_tolerance = has_variance && verdict.above_tolerance;
    // Kept on the line for the PCs that receive it (CMMS Advisor Q10).
    let (entry_unit_cost, entry_variance_value) = verdict.entry_values(variance_qty);
    let (self_approved, pending_approval) = if above_tolerance {
        let allowed = crate::inventory::count_four_eyes::store_rule(tx).await?.rule
            == crate::reliability::study::decisions::SelfApprovalRule::Allowed;
        (allowed, !allowed)
    } else {
        (false, false)
    };
    let now = now_iso();
    let pending_sessions = tx
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            PENDING_LIST_COUNT_SESSIONS,
            [count.article_id.into(), count.location_id.into()],
        ))
        .await?;
    let session_code = next_doc("CC");
    // A waiting count replaced is cancelled, audited on its session: who replaced it, by which count.
    let mut replaced_sessions = Vec::new();
    for row in pending_sessions {
        let replaced_id: i64 = row.try_get("", "session_id")?;
        let replaced = ensure_count_session(tx, replaced_id).await?;
        let written = tx
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inventory_count_sessions SET status = 'cancelled', row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND status = 'submitted'",
                [now.clone().into(), replaced_id.into()],
            ))
            .await?;
        if written.rows_affected() != 1 {
            continue;
        }
        crate::audit::writer::write_audit_event_in(
            tx,
            crate::audit::writer::AuditEventInput {
                action_code: "inventory.count.replaced".to_string(),
                target_type: Some("inventory_count_sessions".to_string()),
                target_id: Some(session_audit_target(tx, replaced_id).await?),
                actor_id: Some(count.actor_id),
                auth_context: "session".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "summary": "Count waiting for approval replaced by a new count of the same article and location (cancelled)",
                    "count_session_code": replaced.session_code,
                    "article_id": count.article_id,
                    "location_id": count.location_id,
                    "replaced_by_session_code": session_code,
                })),
            },
        )
        .await?;
        replaced_sessions.push(replaced.session_code);
    }
    let posted_by = if pending_approval {
        Value::BigInt(None)
    } else {
        Value::from(count.actor_id)
    };
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_count_sessions
            (session_code, warehouse_id, location_id, status, critical_abs_threshold, opened_by_id,
             submitted_by_id, submitted_at, posted_by_id, posted_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?)",
        [
            session_code.clone().into(),
            warehouse_id.into(),
            count.location_id.into(),
            (if pending_approval { "submitted" } else { "posted" }).into(),
            count.actor_id.into(),
            count.actor_id.into(),
            now.clone().into(),
            posted_by.clone(),
            if pending_approval {
                Value::String(None)
            } else {
                Value::from(now.clone())
            },
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
             counted_by_id, self_approved, created_at, updated_at, count_replica_id, count_follows_json, count_path,
             entry_unit_cost, entry_variance_value)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            i64::from(verdict.above_tolerance).into(),
            i64::from(has_variance).into(),
            posted_by,
            if pending_approval {
                Value::String(None)
            } else {
                Value::from(now.clone())
            },
            if pending_approval {
                Value::String(None)
            } else {
                Value::from(count.evidence)
            },
            count.actor_id.into(),
            i64::from(self_approved).into(),
            now.clone().into(),
            now.clone().into(),
            count.mark.0.into(),
            count.mark.1.into(),
            crate::inventory::negative_stock::COUNT_PATH_RECONCILIATION.into(),
            entry_unit_cost.into(),
            entry_variance_value.into(),
        ],
    ))
    .await?;
    let line_id = last_insert_id(tx).await?;
    let transaction_id = if has_variance && !pending_approval {
        Some(
            post_location_line_in(
                tx,
                &LocationLinePosting {
                    session_id,
                    session_code: &session_code,
                    line_id,
                    article_id: count.article_id,
                    warehouse_id,
                    location_id: count.location_id,
                    counted_qty: count.counted_qty,
                    variance_qty,
                    variance_reason_code: count.variance_reason_code,
                    approver_id: count.actor_id,
                    evidence: count.evidence,
                },
            )
            .await?,
        )
    } else {
        None
    };
    Ok(PostedLocationCount {
        session_id,
        session_code,
        line_id,
        system_qty,
        variance_qty,
        transaction_id,
        self_approved,
        pending_approval,
        tolerance_json: verdict.audit_json(),
        replaced_sessions,
    })
}

/// The posting of one list count's variance.
struct LocationLinePosting<'a> {
    session_id: i64,
    session_code: &'a str,
    line_id: i64,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    counted_qty: f64,
    variance_qty: f64,
    variance_reason_code: &'a str,
    approver_id: i64,
    evidence: &'a str,
}

/// Moves the stock of a list count to its counted quantity: the `COUNT_ADJUST` movement of its
/// variance, the line's link to it and the movement's audit link (who approved, with what note).
/// Returns the movement's id.
async fn post_location_line_in<C: ConnectionTrait>(tx: &C, posting: &LocationLinePosting<'_>) -> AppResult<i64> {
    let now = now_iso();
    upsert_balance(
        tx,
        posting.article_id,
        posting.warehouse_id,
        posting.location_id,
        posting.counted_qty,
    )
    .await?;
    let (tx_id, tx_sync_id) = insert_count_transaction(
        tx,
        posting.article_id,
        posting.warehouse_id,
        posting.location_id,
        posting.variance_qty,
        "COUNT_ADJUST",
        posting.session_id,
        posting.session_code,
        Some(posting.variance_reason_code),
        Some(posting.approver_id),
    )
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_lines SET posted_transaction_id = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [tx_id.into(), now.clone().into(), posting.line_id.into()],
    ))
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_mutation_audit_links
            (entity_sync_id, transaction_id, source_entity_type, source_entity_id, reviewer_id, reviewer_evidence, created_at)
         VALUES (?, ?, 'inventory_count_line', ?, ?, ?, ?)",
        [
            count_audit_link_sync_id(&tx_sync_id).into(),
            tx_id.into(),
            posting.line_id.into(),
            posting.approver_id.into(),
            posting.evidence.into(),
            now.into(),
        ],
    ))
    .await?;
    Ok(tx_id)
}

/// A list count waiting for its approval ([`record_location_count_in`]), as read for its approval.
pub(crate) struct PendingLocationCount {
    pub line_id: i64,
    pub row_version: i64,
    pub session_id: i64,
    pub session_code: String,
    pub article_id: i64,
    pub warehouse_id: i64,
    pub location_id: i64,
    pub system_qty: f64,
    pub counted_qty: f64,
    pub variance_qty: f64,
    pub variance_reason_code: Option<String>,
    pub is_critical: bool,
    pub counted_by_id: Option<i64>,
}

/// The list count of this line when it is still waiting for its approval (session `submitted`,
/// line not approved), `None` otherwise.
pub(crate) async fn pending_location_count_in<C: ConnectionTrait>(
    tx: &C,
    line_id: i64,
) -> AppResult<Option<PendingLocationCount>> {
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT cl.id, cl.row_version, cl.session_id, cs.session_code, cl.article_id, cl.warehouse_id,
                    cl.location_id, cl.system_qty, cl.counted_qty, cl.variance_qty, cl.variance_reason_code,
                    cl.is_critical, cl.counted_by_id
             FROM inventory_count_lines cl
             JOIN inventory_count_sessions cs ON cs.id = cl.session_id
             WHERE cl.id = ? AND cl.count_path = 'reconciliation' AND cl.approved_by_id IS NULL
               AND cs.status = 'submitted'",
            [line_id.into()],
        ))
        .await?;
    row.map(|r| -> AppResult<PendingLocationCount> {
        Ok(PendingLocationCount {
            line_id: r.try_get("", "id")?,
            row_version: r.try_get("", "row_version")?,
            session_id: r.try_get("", "session_id")?,
            session_code: r.try_get("", "session_code")?,
            article_id: r.try_get("", "article_id")?,
            warehouse_id: r.try_get("", "warehouse_id")?,
            location_id: r.try_get("", "location_id")?,
            system_qty: r.try_get("", "system_qty")?,
            counted_qty: r.try_get("", "counted_qty")?,
            variance_qty: r.try_get("", "variance_qty")?,
            variance_reason_code: r.try_get("", "variance_reason_code")?,
            is_critical: r.try_get::<i64>("", "is_critical")? != 0,
            counted_by_id: r.try_get("", "counted_by_id")?,
        })
    })
    .transpose()
}

/// Approves and posts a list count waiting for its approval, in the caller's transaction: the line
/// approved by `approver_id` with `evidence` (flagged "auto-approbation" when `self_approved`), the
/// stock moved to the counted quantity and the session posted. The caller checks the right, the
/// note, four-eyes and that the stock on record is still the one the count was set against.
pub(crate) async fn approve_location_count_in<C: ConnectionTrait>(
    tx: &C,
    pending: &PendingLocationCount,
    approver_id: i64,
    evidence: &str,
    self_approved: bool,
) -> AppResult<PostedLocationCount> {
    let now = now_iso();
    let tolerance = crate::inventory::count_four_eyes::evaluate(
        tx,
        pending.article_id,
        pending.location_id,
        pending.variance_qty,
        pending.system_qty,
    )
    .await?;
    let written = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_count_lines
             SET approved_by_id = ?, approved_at = ?, approval_note = ?, self_approved = ?,
                 row_version = row_version + 1, updated_at = ?
             WHERE id = ? AND row_version = ? AND approved_by_id IS NULL",
            [
                approver_id.into(),
                now.clone().into(),
                evidence.into(),
                i64::from(self_approved).into(),
                now.clone().into(),
                pending.line_id.into(),
                pending.row_version.into(),
            ],
        ))
        .await?;
    if written.rows_affected() != 1 {
        return Err(count_line_changed_elsewhere());
    }
    let reason_code = pending.variance_reason_code.clone().unwrap_or_default();
    let transaction_id = post_location_line_in(
        tx,
        &LocationLinePosting {
            session_id: pending.session_id,
            session_code: &pending.session_code,
            line_id: pending.line_id,
            article_id: pending.article_id,
            warehouse_id: pending.warehouse_id,
            location_id: pending.location_id,
            counted_qty: pending.counted_qty,
            variance_qty: pending.variance_qty,
            variance_reason_code: &reason_code,
            approver_id,
            evidence,
        },
    )
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_count_sessions
         SET status = 'posted', posted_by_id = ?, posted_at = ?, row_version = row_version + 1, updated_at = ?
         WHERE id = ? AND status = 'submitted'",
        [
            approver_id.into(),
            now.clone().into(),
            now.into(),
            pending.session_id.into(),
        ],
    ))
    .await?;
    Ok(PostedLocationCount {
        session_id: pending.session_id,
        session_code: pending.session_code.clone(),
        line_id: pending.line_id,
        system_qty: pending.system_qty,
        variance_qty: pending.variance_qty,
        transaction_id: Some(transaction_id),
        self_approved,
        pending_approval: false,
        tolerance_json: approval_tolerance_json(pending.is_critical, &tolerance),
        replaced_sessions: Vec::new(),
    })
}

async fn last_insert_id<C: ConnectionTrait>(db: &C) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec!["GATE_INV_SAVE_FAILED: Could not read the new row id.".to_string()])
        })?;
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
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INV_SAVE_FAILED: Unable to create reconciliation run.".to_string()
            ])
        })?;
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

/// The count line read model, then `tail` (its WHERE / ORDER BY). `self_approved` is derived on
/// read (CMMS Advisor Q13, `count_four_eyes::SELF_APPROVED_ON_READ_SQL`): flagged, or approved by
/// its own counter.
fn count_line_sql(tail: &str) -> String {
    format!(
        "SELECT cl.id, cl.session_id, cl.article_id, a.article_code, a.article_name, cl.warehouse_id, cl.location_id,
                sl.code AS location_code, cl.system_qty, cl.counted_qty, cl.variance_qty, cl.variance_reason_code,
                cl.is_critical, cl.approval_required, cl.approved_by_id,
                COALESCE(ua.display_name, ua.username) AS approved_by_name, cl.approved_at, cl.approval_note,
                cl.counted_by_id, COALESCE(uc.display_name, uc.username) AS counted_by_name,
                {self_approved} AS self_approved,
                cl.second_approved_by_id, COALESCE(us.display_name, us.username) AS second_approved_by_name,
                cl.second_approved_at, cl.second_approval_note,
                cl.posted_transaction_id, cl.reversed_transaction_id, cl.row_version, cl.created_at, cl.updated_at
         FROM inventory_count_lines cl
         JOIN articles a ON a.id = cl.article_id
         JOIN stock_locations sl ON sl.id = cl.location_id
         LEFT JOIN user_accounts ua ON ua.id = cl.approved_by_id
         LEFT JOIN user_accounts uc ON uc.id = cl.counted_by_id
         LEFT JOIN user_accounts us ON us.id = cl.second_approved_by_id
         {tail}",
        self_approved = crate::inventory::count_four_eyes::SELF_APPROVED_ON_READ_SQL,
    )
}

pub async fn list_count_lines(db: &DatabaseConnection, session_id: i64) -> AppResult<Vec<InventoryCountLine>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            count_line_sql("WHERE cl.session_id = ? ORDER BY cl.id ASC"),
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

async fn get_count_line<C: ConnectionTrait>(db: &C, line_id: i64) -> AppResult<InventoryCountLine> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            count_line_sql("WHERE cl.id = ?"),
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
        counted_by_id: row.try_get("", "counted_by_id")?,
        counted_by_name: row.try_get("", "counted_by_name").ok().flatten(),
        self_approved: row.try_get("", "self_approved")?,
        second_approved_by_id: row.try_get("", "second_approved_by_id")?,
        second_approved_by_name: row.try_get("", "second_approved_by_name").ok().flatten(),
        second_approved_at: row.try_get("", "second_approved_at")?,
        second_approval_note: row.try_get("", "second_approval_note")?,
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

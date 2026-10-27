//! Work on retired equipment received by sync: "équipement déclassé — à revoir" (audit finding
//! AU-73b, CMMS Advisor decision).
//!
//! On one PC no work order or request is created on retired equipment (Déclassé / Rebuté:
//! `wo::queries::refuse_retired_equipment`, `di::queries::refuse_retired_equipment`), and equipment
//! with an open work order or request is not retired (`assets::decommission`). Two PCs can still
//! meet in that state: a work order or request created on one PC while the equipment was in service
//! there, and the retirement recorded on another. Whichever arrives second, the received row is a
//! field fact: it is applied, never refused or lost.
//!
//! Such an open work order or request on retired equipment is one review item, per work order or
//! request ([`ReviewKey`]: its sync id, the same on every PC). The item is a projection of
//! synchronised state, never a row invented here: it is open exactly while
//! - the work order is not closed or cancelled (`wo::backlog::NOT_CLOSED_SQL`), or the request is
//!   open (`di::stats::OPEN_SQL`), the rules the retirement gate itself reads; and
//! - its equipment's lifecycle status is retired (`assets::taxonomy_reference::RETIRED_STATUS_CODES_SQL`,
//!   the rule of `assets::identity::is_retired_status_code`).
//!
//! It therefore covers both orders of arrival (the work received after the retirement, or the
//! retirement received after the work), and every PC holding the same rows shows the same open
//! items. The items are this PC's projection (`retired_equipment_reviews`, not synchronised:
//! `sync::registry`, derived projection); [`reconcile_in`] opens or closes one after each received
//! work order, request or equipment row (`sync::apply_hooks`), and before the list is read the
//! items that differ from the state are reconciled ([`pending_keys`]: local actions are caught there;
//! a read that finds nothing to change takes no write lock).
//!
//! Times, the same on every PC. An item's `opened_at` is when both became true: the later of the
//! record's creation and the retirement it was opened under (`retired_at`: the equipment's
//! `decommissioned_at`, else its latest retirement event), kept up to date while it is open (a
//! return to service and a new retirement may reach this PC as one equipment row). How, when and
//! by whom a closed item closed is resolved when the list is read: the earlier of the record's own
//! end and the return to service ending that retirement (`list_select`), so the arrival order of
//! the two never changes it. A closure keeps the record's own time
//! (its close, cancel or conversion time; for a record gone or moved, its last change). A return to
//! service is the FIRST `RECOMMISSIONED` event at or after `retired_at`, read when the list is read
//! (time, who, note), so an event that arrives after the equipment row, or a later retire /
//! recommission cycle, never changes which one it names; the time stored when it closed is only
//! the fallback while that event is not here yet.
//!
//! Who closed it and their note are read from the synchronised transition that ended the record
//! (the latest one into its terminal status) or that return to service, when the list is read.
//!
//! Settlement uses only the existing actions: cancel the work order or reject the request with a
//! reason, or reverse the retirement through the normal lifecycle right. There is no dismiss.
//!
//! Notice. Two watchers, one per module, so each is seen only with that module's view right
//! (`notifications::inbox`, category module): [`NOTIFICATION_CATEGORY`] (work orders, module `ot`:
//! the planner and the responsibles of the work order's node, the equipment's site) and
//! [`REQUEST_NOTIFICATION_CATEGORY`] (requests, module `di`: the reviewer and the responsibles of
//! the request's node). Audiences are their rules' (Données de référence). Each is told once per
//! item, in-app, with the same identity on every PC.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

use crate::auth::scope::{scope_filter_sql, AccessScope};
use crate::errors::{AppError, AppResult};
use crate::wo::workflow::state_machine::WoStatus;

/// Notification category (and watcher) of the work orders on retired equipment (migration 265,
/// module `ot`).
pub const NOTIFICATION_CATEGORY: &str = "retired_equipment_work";
/// Notification category (and watcher) of the requests on retired equipment (migration 265,
/// module `di`).
pub const REQUEST_NOTIFICATION_CATEGORY: &str = "retired_equipment_request";

/// A work order item.
pub const KIND_WORK_ORDER: &str = "work_order";
/// An intervention request item.
pub const KIND_REQUEST: &str = "intervention_request";

/// The work order was closed: "clos par clôture de l'OT".
pub const CLOSURE_WO_CLOSED: &str = "wo_closed";
/// The work order was cancelled.
pub const CLOSURE_WO_CANCELLED: &str = "wo_cancelled";
/// The request was rejected (`rejected_invalid`, `duplicate`).
pub const CLOSURE_DI_REJECTED: &str = "di_rejected";
/// The request was closed with another disposition.
pub const CLOSURE_DI_CLOSED: &str = "di_closed";
/// The request was converted to a work order (which carries its own item while open).
pub const CLOSURE_DI_CONVERTED: &str = "di_converted";
/// The equipment was returned to service.
pub const CLOSURE_RECOMMISSIONED: &str = "recommissioned";
/// Neither: the record is gone, or now names another equipment in service.
pub const CLOSURE_OTHER: &str = "other";

/// Most closed items listed (latest closure first): the history is read on demand, bounded.
pub const HISTORY_LIMIT: i64 = 100;

/// Request dispositions read as a rejection (`di::review::reject_di`).
const REJECTED_DISPOSITIONS: &[&str] = &[
    crate::di::disposition::DISPOSITION_REJECTED_INVALID,
    crate::di::disposition::DISPOSITION_DUPLICATE,
];

/// The item's identity, the same on every PC: the record's kind and sync id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReviewKey {
    pub kind: &'static str,
    pub sync_id: String,
}

impl ReviewKey {
    pub fn work_order(sync_id: impl Into<String>) -> Self {
        Self {
            kind: KIND_WORK_ORDER,
            sync_id: sync_id.into(),
        }
    }

    pub fn request(sync_id: impl Into<String>) -> Self {
        Self {
            kind: KIND_REQUEST,
            sync_id: sync_id.into(),
        }
    }

    fn of(kind: &str, sync_id: String) -> Self {
        if kind == KIND_WORK_ORDER {
            Self::work_order(sync_id)
        } else {
            Self::request(sync_id)
        }
    }

    /// `wo|<sync id>` or `di|<sync id>`: the item key and the notice's occurrence.
    pub fn key(&self) -> String {
        let prefix = if self.kind == KIND_WORK_ORDER { "wo" } else { "di" };
        format!("{prefix}|{}", self.sync_id)
    }
}

/// One item, open or closed, with its record and equipment as they stand here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetiredEquipmentReview {
    pub id: i64,
    /// [`ReviewKey::key`].
    pub review_key: String,
    /// [`KIND_WORK_ORDER`] or [`KIND_REQUEST`].
    pub kind: String,
    /// The record here (`None` when it is no longer here).
    pub record_id: Option<i64>,
    pub record_code: Option<String>,
    pub record_title: Option<String>,
    /// Work-order status code, or request status.
    pub record_status: Option<String>,
    pub equipment_id: i64,
    pub equipment_code: Option<String>,
    pub equipment_name: Option<String>,
    pub equipment_status: Option<String>,
    /// `open` or `closed`.
    pub status: String,
    /// When both became true: the later of the record's creation and the retirement.
    pub opened_at: String,
    pub closed_at: Option<String>,
    /// One of the `CLOSURE_*` codes.
    pub closure_kind: Option<String>,
    /// Who closed it (the transition or return to service that ended it), when known here.
    pub closed_by: Option<String>,
    /// Its note (cancel reason, closing note, recommission reason).
    pub closure_note: Option<String>,
    /// A converted request: the work order it became.
    pub converted_to_wo_id: Option<i64>,
    pub converted_to_wo_code: Option<String>,
    /// A closed request: its disposition.
    pub disposition_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetiredEquipmentReviewList {
    /// Open items (latest first), then, when asked for, the latest closed ones (at most
    /// [`HISTORY_LIMIT`], latest closure first).
    pub items: Vec<RetiredEquipmentReview>,
    /// Closed items the reader may see, all of them (the history may list fewer).
    pub closed_count: i64,
}

fn sqlite(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn decode(col: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("retired_equipment_reviews decode '{col}': {e}"))
}

fn opt_s(r: &QueryResult, col: &str) -> AppResult<Option<String>> {
    r.try_get::<Option<String>>("", col).map_err(|e| decode(col, e))
}

fn opt_i(r: &QueryResult, col: &str) -> AppResult<Option<i64>> {
    r.try_get::<Option<i64>>("", col).map_err(|e| decode(col, e))
}

/// SQL predicate (alias `e`, no bind): the equipment is here and its lifecycle status is retired.
pub(crate) fn retired_sql() -> String {
    format!(
        "(e.id IS NOT NULL AND e.deleted_at IS NULL AND e.lifecycle_status IN ({}))",
        crate::assets::taxonomy_reference::RETIRED_STATUS_CODES_SQL
    )
}

/// SQL expression (alias `e`, no bind): when the equipment was retired, as the same on every PC:
/// its `decommissioned_at`, else its latest retirement event.
const RETIRED_AT_SQL: &str =
    "COALESCE(e.decommissioned_at, (SELECT MAX(ev.occurred_at) FROM equipment_lifecycle_events ev \
     WHERE ev.equipment_id = e.id AND ev.event_type = 'DECOMMISSIONED'))";

/// The open work orders on retired equipment (aliases `wo`, `s`, `e`).
pub(crate) fn open_work_orders_sql(columns: &str) -> String {
    format!(
        "SELECT {columns} FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         JOIN equipment e ON e.id = wo.equipment_id \
         WHERE wo.entity_sync_id IS NOT NULL AND {} AND {}",
        crate::wo::backlog::NOT_CLOSED_SQL,
        retired_sql()
    )
}

/// The open requests on retired equipment (aliases `ir`, `e`).
pub(crate) fn open_requests_sql(columns: &str) -> String {
    format!(
        "SELECT {columns} FROM intervention_requests ir JOIN equipment e ON e.id = ir.equipment_id \
         WHERE ir.entity_sync_id IS NOT NULL AND {} AND {}",
        crate::di::stats::OPEN_SQL,
        retired_sql()
    )
}

/// The later of two stored times (either may be missing or unreadable).
fn later(a: Option<String>, b: Option<String>) -> Option<String> {
    use crate::notifications::watchers::parse_time;
    match (a, b) {
        (Some(a), Some(b)) => match (parse_time(&a), parse_time(&b)) {
            (Some(ta), Some(tb)) if tb > ta => Some(b),
            (Some(_), _) => Some(a),
            (None, _) => Some(b),
        },
        (a, b) => a.or(b),
    }
}

/// How a work order in a terminal status ended (the two codes of `wo::backlog::CLOSED_STATUS_SQL`).
fn wo_end_kind(status: &str) -> &'static str {
    if status == WoStatus::Cancelled.as_str() {
        CLOSURE_WO_CANCELLED
    } else {
        CLOSURE_WO_CLOSED
    }
}

/// How a request no longer open ended: converted, rejected, or closed otherwise.
fn di_end_kind(status: &str, disposition: &str, converted: bool) -> &'static str {
    if converted
        || disposition == crate::di::disposition::DISPOSITION_CONVERTED_TO_WO
        || status == "converted_to_work_order"
    {
        CLOSURE_DI_CONVERTED
    } else if REJECTED_DISPOSITIONS.contains(&disposition) || status == "rejected" {
        CLOSURE_DI_REJECTED
    } else {
        CLOSURE_DI_CLOSED
    }
}

/// The state of one record here, as the item reads it.
struct RecordState {
    record_id: i64,
    equipment_id: Option<i64>,
    /// Its equipment is here and retired.
    retired: bool,
    /// Its equipment is here and not retired.
    in_service: bool,
    /// The closure its own state gives (terminal), with the record's time of it.
    terminal: Option<(&'static str, Option<String>)>,
    created_at: Option<String>,
    updated_at: Option<String>,
    /// When its equipment was retired ([`RETIRED_AT_SQL`]), while retired.
    retired_at: Option<String>,
}

async fn record_state(conn: &impl ConnectionTrait, key: &ReviewKey) -> AppResult<Option<RecordState>> {
    let retired = retired_sql();
    let row = if key.kind == KIND_WORK_ORDER {
        conn.query_one(sqlite(
            &format!(
                "SELECT wo.id, wo.equipment_id, s.code AS status, wo.closed_at, wo.cancelled_at, wo.created_at, \
                        wo.updated_at, CASE WHEN s.code IN {} THEN 1 ELSE 0 END AS terminal, \
                        CASE WHEN {retired} THEN 1 ELSE 0 END AS retired, \
                        CASE WHEN e.id IS NOT NULL AND e.deleted_at IS NULL THEN 1 ELSE 0 END AS present, \
                        CASE WHEN {retired} THEN {RETIRED_AT_SQL} END AS retired_at \
                 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 LEFT JOIN equipment e ON e.id = wo.equipment_id WHERE wo.entity_sync_id = ?",
                crate::wo::backlog::CLOSED_STATUS_SQL
            ),
            vec![key.sync_id.clone().into()],
        ))
        .await?
    } else {
        conn.query_one(sqlite(
            &format!(
                "SELECT ir.id, ir.equipment_id, ir.status, ir.disposition_code, ir.closed_at, ir.declined_at, \
                        ir.converted_at, ir.created_at, ir.updated_at, \
                        CASE WHEN {} THEN 0 ELSE 1 END AS terminal, \
                        CASE WHEN {retired} THEN 1 ELSE 0 END AS retired, \
                        CASE WHEN e.id IS NOT NULL AND e.deleted_at IS NULL THEN 1 ELSE 0 END AS present, \
                        CASE WHEN {retired} THEN {RETIRED_AT_SQL} END AS retired_at \
                 FROM intervention_requests ir LEFT JOIN equipment e ON e.id = ir.equipment_id \
                 WHERE ir.entity_sync_id = ?",
                crate::di::stats::OPEN_SQL
            ),
            vec![key.sync_id.clone().into()],
        ))
        .await?
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let retired = opt_i(&row, "retired")?.unwrap_or(0) == 1;
    let present = opt_i(&row, "present")?.unwrap_or(0) == 1;
    let is_terminal = opt_i(&row, "terminal")?.unwrap_or(0) == 1;
    let updated_at = opt_s(&row, "updated_at")?;
    let terminal = if !is_terminal {
        None
    } else if key.kind == KIND_WORK_ORDER {
        // The two codes of `wo::backlog::CLOSED_STATUS_SQL`: cancelled, or closed.
        let kind = wo_end_kind(&opt_s(&row, "status")?.unwrap_or_default());
        let at = if kind == CLOSURE_WO_CANCELLED {
            opt_s(&row, "cancelled_at")?
        } else {
            opt_s(&row, "closed_at")?
        };
        Some((kind, at.or(updated_at.clone())))
    } else {
        let status = opt_s(&row, "status")?.unwrap_or_default();
        let disposition = opt_s(&row, "disposition_code")?.unwrap_or_default();
        let converted_at = opt_s(&row, "converted_at")?;
        let kind = di_end_kind(&status, &disposition, converted_at.is_some());
        // The time of `END_AT_SQL`: conversion, close, decline, last change.
        let at = converted_at
            .or(opt_s(&row, "closed_at")?)
            .or(opt_s(&row, "declined_at")?)
            .or(updated_at.clone());
        Some((kind, at))
    };
    Ok(Some(RecordState {
        record_id: row.try_get("", "id").map_err(|e| decode("id", e))?,
        equipment_id: opt_i(&row, "equipment_id")?,
        retired,
        in_service: present && !retired,
        terminal,
        created_at: opt_s(&row, "created_at")?,
        updated_at,
        retired_at: opt_s(&row, "retired_at")?,
    }))
}

/// SQL predicate (alias `ev`; `{equipment}` and `{since}` are SQL written here, never user input):
/// the `RECOMMISSIONED` events of the equipment that end a retirement that began at `since` — those
/// that happened at or after it, or were recorded at or after it (a return to service entered with
/// an earlier date still counts: it cannot end a retirement it was recorded before).
fn recommission_after_sql(equipment: &str, since: &str) -> String {
    format!(
        "ev.equipment_id = {equipment} AND ev.event_type = 'RECOMMISSIONED' \
         AND MAX(julianday(ev.occurred_at), COALESCE(julianday(ev.created_at), julianday(ev.occurred_at))) \
             >= julianday({since})"
    )
}

/// When the equipment was returned to service after `since` (the item's retirement, else its
/// opening): the first such event, ties by sync id — the same event on every PC.
async fn recommissioned_at(conn: &impl ConnectionTrait, equipment_id: i64, since: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_one(sqlite(
            &format!(
                "SELECT ev.occurred_at FROM equipment_lifecycle_events ev WHERE {} \
                 ORDER BY julianday(ev.occurred_at), ev.entity_sync_id LIMIT 1",
                recommission_after_sql("?", "?")
            ),
            vec![equipment_id.into(), since.into()],
        ))
        .await?
        .map(|r| opt_s(&r, "occurred_at"))
        .transpose()?
        .flatten())
}

/// How an open item closes, and when: its record's terminal state first (its own time), else its
/// equipment returned to service (that event's time; read again when listed), else
/// [`CLOSURE_OTHER`] (the record's last change; now when the record is gone).
async fn closure(
    conn: &impl ConnectionTrait,
    state: Option<&RecordState>,
    item_equipment_id: i64,
    item_since: &str,
) -> AppResult<(&'static str, String)> {
    let Some(state) = state else {
        return Ok((CLOSURE_OTHER, now_iso()));
    };
    if let Some((kind, at)) = &state.terminal {
        return Ok((*kind, at.clone().unwrap_or_else(now_iso)));
    }
    if state.in_service && state.equipment_id == Some(item_equipment_id) {
        let at = recommissioned_at(conn, item_equipment_id, item_since).await?;
        return Ok((CLOSURE_RECOMMISSIONED, at.unwrap_or_else(now_iso)));
    }
    Ok((CLOSURE_OTHER, state.updated_at.clone().unwrap_or_else(now_iso)))
}

/// Opens or closes the item of one work order or request from its state here: open while the
/// record is open and its equipment retired (one open item per record), closed otherwise with how
/// and when.
pub async fn reconcile_in(conn: &impl ConnectionTrait, key: &ReviewKey) -> AppResult<()> {
    let state = record_state(conn, key).await?;
    let open_now = state
        .as_ref()
        .is_some_and(|s| s.retired && s.terminal.is_none() && s.equipment_id.is_some());
    let item = conn
        .query_one(sqlite(
            "SELECT id, equipment_id, retired_at, opened_at FROM retired_equipment_reviews \
             WHERE record_kind = ? AND record_sync_id = ? AND status = 'open'",
            vec![key.kind.into(), key.sync_id.clone().into()],
        ))
        .await?;
    match (open_now, item, state) {
        (true, None, Some(state)) => {
            let opened_at = later(state.created_at.clone(), state.retired_at.clone()).unwrap_or_else(now_iso);
            // A reconcile running at the same time may have opened it: one open row per record.
            conn.execute(sqlite(
                "INSERT INTO retired_equipment_reviews \
                    (record_kind, record_sync_id, record_id, equipment_id, status, opened_at, retired_at) \
                 VALUES (?, ?, ?, ?, 'open', ?, ?) \
                 ON CONFLICT(record_kind, record_sync_id) WHERE status = 'open' DO NOTHING",
                vec![
                    key.kind.into(),
                    key.sync_id.clone().into(),
                    state.record_id.into(),
                    state.equipment_id.into(),
                    opened_at.into(),
                    state.retired_at.clone().into(),
                ],
            ))
            .await?;
        }
        (true, Some(item), Some(state)) => {
            // Still open: it follows the record's equipment (moved to another retired one) and the
            // retirement in force (a return to service and a new retirement this PC never saw as
            // such: only the latest equipment row arrived), with the opening that goes with it.
            let id: i64 = item.try_get("", "id").map_err(|e| decode("id", e))?;
            let opened_at = later(state.created_at.clone(), state.retired_at.clone()).unwrap_or_else(now_iso);
            conn.execute(sqlite(
                "UPDATE retired_equipment_reviews SET equipment_id = ?, record_id = ?, retired_at = ?, opened_at = ? \
                 WHERE id = ? AND status = 'open' AND (equipment_id IS NOT ? OR retired_at IS NOT ?)",
                vec![
                    state.equipment_id.into(),
                    state.record_id.into(),
                    state.retired_at.clone().into(),
                    opened_at.into(),
                    id.into(),
                    state.equipment_id.into(),
                    state.retired_at.clone().into(),
                ],
            ))
            .await?;
        }
        (false, Some(item), state) => {
            let id: i64 = item.try_get("", "id").map_err(|e| decode("id", e))?;
            let equipment_id: i64 = item
                .try_get("", "equipment_id")
                .map_err(|e| decode("equipment_id", e))?;
            let since = opt_s(&item, "retired_at")?
                .or(opt_s(&item, "opened_at")?)
                .unwrap_or_default();
            let (kind, at) = closure(conn, state.as_ref(), equipment_id, &since).await?;
            conn.execute(sqlite(
                "UPDATE retired_equipment_reviews SET status = 'closed', closed_at = ?, closure_kind = ? \
                 WHERE id = ? AND status = 'open'",
                vec![at.into(), kind.into(), id.into()],
            ))
            .await?;
        }
        _ => {}
    }
    Ok(())
}

async fn sync_id_of(conn: &impl ConnectionTrait, table: &str, id: i64) -> AppResult<Option<String>> {
    Ok(conn
        .query_one(sqlite(
            &format!("SELECT entity_sync_id FROM {table} WHERE id = ?"),
            vec![id.into()],
        ))
        .await?
        .map(|r| opt_s(&r, "entity_sync_id"))
        .transpose()?
        .flatten())
}

/// [`reconcile_in`] for a work order applied here (`sync::apply_hooks`).
pub async fn reconcile_work_order_in(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    match sync_id_of(conn, "work_orders", wo_id).await? {
        Some(sid) => reconcile_in(conn, &ReviewKey::work_order(sid)).await,
        None => Ok(()),
    }
}

/// [`reconcile_in`] for a request applied here (`sync::apply_hooks`).
pub async fn reconcile_request_in(conn: &impl ConnectionTrait, di_id: i64) -> AppResult<()> {
    match sync_id_of(conn, "intervention_requests", di_id).await? {
        Some(sid) => reconcile_in(conn, &ReviewKey::request(sid)).await,
        None => Ok(()),
    }
}

async fn keys_of(conn: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> AppResult<Vec<ReviewKey>> {
    let rows = conn.query_all(sqlite(sql, values)).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let kind = opt_s(&r, "kind")?.unwrap_or_default();
        let Some(sid) = opt_s(&r, "sid")? else { continue };
        out.push(ReviewKey::of(&kind, sid));
    }
    Ok(out)
}

/// The OPEN work orders and requests of one equipment, and its open items, after the equipment row
/// was applied here (a retirement or a return to service received by sync). Closed records have no
/// open item and open none: they are not read.
pub async fn reconcile_equipment_in(conn: &impl ConnectionTrait, equipment_id: i64) -> AppResult<()> {
    let retired = conn
        .query_one(sqlite(
            &format!("SELECT 1 AS hit FROM equipment e WHERE e.id = ? AND {}", retired_sql()),
            vec![equipment_id.into()],
        ))
        .await?
        .is_some();
    if !retired {
        // Nothing opens on equipment in service: only its open items can close.
        let keys = keys_of(
            conn,
            "SELECT record_kind AS kind, record_sync_id AS sid FROM retired_equipment_reviews \
             WHERE equipment_id = ? AND status = 'open'",
            vec![equipment_id.into()],
        )
        .await?;
        for key in keys {
            reconcile_in(conn, &key).await?;
        }
        return Ok(());
    }
    let keys = keys_of(
        conn,
        &format!(
            "SELECT '{KIND_WORK_ORDER}' AS kind, wo.entity_sync_id AS sid FROM work_orders wo \
               JOIN work_order_statuses s ON s.id = wo.status_id WHERE wo.equipment_id = ? AND {} \
             UNION SELECT '{KIND_REQUEST}', ir.entity_sync_id FROM intervention_requests ir \
               WHERE ir.equipment_id = ? AND {} \
             UNION SELECT record_kind, record_sync_id FROM retired_equipment_reviews \
                   WHERE equipment_id = ? AND status = 'open'",
            crate::wo::backlog::NOT_CLOSED_SQL,
            crate::di::stats::OPEN_SQL,
        ),
        vec![equipment_id.into(), equipment_id.into(), equipment_id.into()],
    )
    .await?;
    for key in keys {
        reconcile_in(conn, &key).await?;
    }
    Ok(())
}

/// The records whose item differs from their state (read only): open on retired equipment without
/// an open item, or an open item whose record is no longer open on that retired equipment.
pub async fn pending_keys(conn: &impl ConnectionTrait) -> AppResult<Vec<ReviewKey>> {
    let no_item = |kind: &str, sid: &str| {
        format!(
            "NOT EXISTS (SELECT 1 FROM retired_equipment_reviews r WHERE r.record_kind = '{kind}' \
             AND r.record_sync_id = {sid} AND r.status = 'open')"
        )
    };
    let sql = format!(
        "{wo_missing} AND {wo_no_item} \
         UNION {di_missing} AND {di_no_item} \
         UNION SELECT r.record_kind, r.record_sync_id FROM retired_equipment_reviews r \
               WHERE r.status = 'open' AND NOT ( \
                 (r.record_kind = '{KIND_WORK_ORDER}' AND EXISTS ({wo_held} \
                    AND wo.entity_sync_id = r.record_sync_id AND wo.equipment_id = r.equipment_id)) \
                 OR (r.record_kind = '{KIND_REQUEST}' AND EXISTS ({di_held} \
                    AND ir.entity_sync_id = r.record_sync_id AND ir.equipment_id = r.equipment_id)))",
        wo_missing = open_work_orders_sql(&format!("'{KIND_WORK_ORDER}' AS kind, wo.entity_sync_id AS sid")),
        wo_no_item = no_item(KIND_WORK_ORDER, "wo.entity_sync_id"),
        di_missing = open_requests_sql(&format!("'{KIND_REQUEST}', ir.entity_sync_id")),
        di_no_item = no_item(KIND_REQUEST, "ir.entity_sync_id"),
        wo_held = open_work_orders_sql("1"),
        di_held = open_requests_sql("1"),
    );
    keys_of(conn, &sql, vec![]).await
}

/// [`reconcile_in`] for every record [`pending_keys`] names.
pub async fn reconcile_all_in(conn: &impl ConnectionTrait) -> AppResult<()> {
    for key in pending_keys(conn).await? {
        reconcile_in(conn, &key).await?;
    }
    Ok(())
}

/// The list: each item with its record, equipment, and the candidates of its closure, resolved
/// when read so that every PC holding the same rows shows the same how / when / who whatever order
/// they arrived in (one statement, no query per row):
/// - the record's end, when it is terminal now (`END_AT_SQL`, its own time; who: the FIRST
///   transition into that terminal status, ties by sync id);
/// - the equipment's return to service ending the retirement the item was opened under (the first
///   `RECOMMISSIONED` event after `retired_at`, else after `opened_at`, [`recommission_after_sql`]).
///
/// The earlier of the two closed it (a tie: the record's own end); the stored closure is only the
/// fallback while neither is here. `resolved_at` orders the history.
fn list_select() -> String {
    let end_at = format!(
        "(CASE WHEN r.record_kind = 'work_order' THEN (CASE WHEN s.code = '{cancelled}' \
              THEN COALESCE(wo.cancelled_at, wo.updated_at) \
              WHEN s.code IN {closed} THEN COALESCE(wo.closed_at, wo.updated_at) END) \
          WHEN ir.id IS NOT NULL AND NOT {di_open} \
              THEN COALESCE(ir.converted_at, ir.closed_at, ir.declined_at, ir.updated_at) END)",
        cancelled = WoStatus::Cancelled.as_str(),
        closed = crate::wo::backlog::CLOSED_STATUS_SQL,
        di_open = crate::di::stats::OPEN_SQL,
    );
    let rec_first = format!(
        "(rev.occurred_at IS NOT NULL AND ({end_at} IS NULL OR julianday(rev.occurred_at) < julianday({end_at})))"
    );
    format!(
        "SELECT r.id, r.record_kind, r.record_sync_id, r.equipment_id, r.status, r.opened_at, \
            r.closure_kind AS stored_kind, r.closed_at AS stored_closed_at, \
            {end_at} AS end_at, CASE WHEN {rec_first} THEN 1 ELSE 0 END AS rec_first, \
            CASE WHEN r.status = 'open' THEN NULL WHEN {rec_first} THEN rev.occurred_at \
                 ELSE COALESCE({end_at}, r.closed_at) END AS resolved_at, \
            rev.occurred_at AS rec_at, rua.display_name AS rec_by, rev.notes AS rec_note, \
            wua.display_name AS wo_by, \
            CASE WHEN s.code = '{cancelled}' THEN COALESCE(wl.notes, wo.cancel_reason) ELSE wl.notes END AS wo_note, \
            dua.display_name AS di_by, dl.notes AS di_note, \
            CASE r.record_kind WHEN 'work_order' THEN wo.id ELSE ir.id END AS record_id, \
            CASE r.record_kind WHEN 'work_order' THEN wo.code ELSE ir.code END AS record_code, \
            CASE r.record_kind WHEN 'work_order' THEN wo.title ELSE ir.title END AS record_title, \
            CASE r.record_kind WHEN 'work_order' THEN s.code ELSE ir.status END AS record_status, \
            ir.disposition_code, ir.converted_at, ir.converted_to_wo_id, cwo.code AS converted_to_wo_code, \
            e.asset_id_code AS equipment_code, e.name AS equipment_name, e.lifecycle_status AS equipment_status \
         FROM retired_equipment_reviews r \
         LEFT JOIN work_orders wo ON r.record_kind = 'work_order' AND wo.entity_sync_id = r.record_sync_id \
         LEFT JOIN work_order_statuses s ON s.id = wo.status_id \
         LEFT JOIN intervention_requests ir ON r.record_kind = 'intervention_request' \
              AND ir.entity_sync_id = r.record_sync_id \
         LEFT JOIN work_orders cwo ON cwo.id = ir.converted_to_wo_id \
         LEFT JOIN equipment e ON e.id = r.equipment_id \
         LEFT JOIN wo_state_transition_log wl ON r.status = 'closed' AND wl.id = ( \
            SELECT l.id FROM wo_state_transition_log l \
            WHERE l.wo_id = wo.id AND s.code IN {closed} AND l.to_status = s.code \
            ORDER BY l.acted_at, l.entity_sync_id LIMIT 1) \
         LEFT JOIN user_accounts wua ON wua.id = wl.actor_id \
         LEFT JOIN di_state_transition_log dl ON r.status = 'closed' AND dl.id = ( \
            SELECT l.id FROM di_state_transition_log l WHERE l.di_id = ir.id AND l.to_status IN {di_terminal} \
            ORDER BY l.acted_at, l.entity_sync_id LIMIT 1) \
         LEFT JOIN user_accounts dua ON dua.id = dl.actor_id \
         LEFT JOIN equipment_lifecycle_events rev ON r.status = 'closed' AND rev.id = ( \
            SELECT ev.id FROM equipment_lifecycle_events ev WHERE {rec_after} \
            ORDER BY julianday(ev.occurred_at), ev.entity_sync_id LIMIT 1) \
         LEFT JOIN user_accounts rua ON rua.id = rev.performed_by_id",
        cancelled = WoStatus::Cancelled.as_str(),
        closed = crate::wo::backlog::CLOSED_STATUS_SQL,
        di_terminal = crate::di::stats::TERMINAL_STATUSES_SQL,
        rec_after = recommission_after_sql("r.equipment_id", "COALESCE(r.retired_at, r.opened_at)"),
    )
}

fn map_item(r: &QueryResult) -> AppResult<RetiredEquipmentReview> {
    let kind = opt_s(r, "record_kind")?.unwrap_or_default();
    let key = ReviewKey::of(&kind, opt_s(r, "record_sync_id")?.unwrap_or_default());
    let is_request = kind == KIND_REQUEST;
    let only_request = |v: Option<String>| if is_request { v } else { None };
    let closed = opt_s(r, "status")?.as_deref() == Some("closed");
    // The closure as resolved now (see `list_select`); the stored one while neither is here.
    let (closure_kind, closed_by, closure_note) = if !closed {
        (None, None, None)
    } else if opt_i(r, "rec_first")? == Some(1) {
        (
            Some(CLOSURE_RECOMMISSIONED.to_string()),
            opt_s(r, "rec_by")?,
            opt_s(r, "rec_note")?,
        )
    } else if opt_s(r, "end_at")?.is_some() {
        let status = opt_s(r, "record_status")?.unwrap_or_default();
        if is_request {
            let disposition = opt_s(r, "disposition_code")?.unwrap_or_default();
            let converted = opt_s(r, "converted_at")?.is_some();
            (
                Some(di_end_kind(&status, &disposition, converted).to_string()),
                opt_s(r, "di_by")?,
                opt_s(r, "di_note")?,
            )
        } else {
            (
                Some(wo_end_kind(&status).to_string()),
                opt_s(r, "wo_by")?,
                opt_s(r, "wo_note")?,
            )
        }
    } else {
        (opt_s(r, "stored_kind")?, None, None)
    };
    Ok(RetiredEquipmentReview {
        id: r.try_get("", "id").map_err(|e| decode("id", e))?,
        review_key: key.key(),
        kind,
        record_id: opt_i(r, "record_id")?,
        record_code: opt_s(r, "record_code")?,
        record_title: opt_s(r, "record_title")?,
        record_status: opt_s(r, "record_status")?,
        equipment_id: r.try_get("", "equipment_id").map_err(|e| decode("equipment_id", e))?,
        equipment_code: opt_s(r, "equipment_code")?,
        equipment_name: opt_s(r, "equipment_name")?,
        equipment_status: opt_s(r, "equipment_status")?,
        status: opt_s(r, "status")?.unwrap_or_default(),
        opened_at: opt_s(r, "opened_at")?.unwrap_or_default(),
        closed_at: if closed { opt_s(r, "resolved_at")? } else { None },
        closure_kind,
        closed_by,
        closure_note,
        converted_to_wo_id: if is_request {
            opt_i(r, "converted_to_wo_id")?
        } else {
            None
        },
        converted_to_wo_code: only_request(opt_s(r, "converted_to_wo_code")?),
        disposition_code: only_request(opt_s(r, "disposition_code")?),
    })
}

/// The items the reader may see: work orders where they hold the work-order view right
/// (`wo_scope`, on the work order's org node), requests where they hold the request view right
/// (`di_scope`, on the request's node); the open ones, and with `include_closed` the latest
/// [`HISTORY_LIMIT`] closed ones. The items that differ from the state are reconciled first, in a
/// write transaction only when there is one.
pub async fn list_reviews(
    db: &DatabaseConnection,
    wo_scope: &AccessScope,
    di_scope: &AccessScope,
    include_closed: bool,
) -> AppResult<RetiredEquipmentReviewList> {
    if !pending_keys(db).await?.is_empty() {
        let tx = db.begin().await?;
        reconcile_all_in(&tx).await?;
        tx.commit().await?;
    }

    let (wo_pred, wo_binds) = scope_filter_sql(wo_scope, crate::wo::access::WO_ANCHOR_SQL);
    let (di_pred, di_binds) = scope_filter_sql(di_scope, crate::di::access::DI_ANCHOR_SQL);
    let visible = format!(
        "((r.record_kind = 'work_order' AND {wo_pred}) OR (r.record_kind = 'intervention_request' AND {di_pred}))"
    );
    let mut binds = wo_binds;
    binds.extend(di_binds);
    let list_select = list_select();

    let open = db
        .query_all(sqlite(
            &format!("{list_select} WHERE {visible} AND r.status = 'open' ORDER BY r.opened_at DESC, r.id DESC"),
            binds.clone(),
        ))
        .await?;
    let mut items = open.iter().map(map_item).collect::<AppResult<Vec<_>>>()?;
    let closed_count = db
        .query_one(sqlite(
            &format!(
                "SELECT COUNT(*) AS c FROM retired_equipment_reviews r \
                 LEFT JOIN work_orders wo ON r.record_kind = 'work_order' AND wo.entity_sync_id = r.record_sync_id \
                 LEFT JOIN intervention_requests ir ON r.record_kind = 'intervention_request' \
                      AND ir.entity_sync_id = r.record_sync_id \
                 WHERE {visible} AND r.status = 'closed'"
            ),
            binds.clone(),
        ))
        .await?
        .map(|r| opt_i(&r, "c"))
        .transpose()?
        .flatten()
        .unwrap_or(0);
    if include_closed {
        let mut values = binds;
        values.push(HISTORY_LIMIT.into());
        let closed = db
            .query_all(sqlite(
                &format!(
                    "{list_select} WHERE {visible} AND r.status = 'closed' \
                     ORDER BY julianday(resolved_at) DESC, r.id DESC LIMIT ?"
                ),
                values,
            ))
            .await?;
        for r in &closed {
            items.push(map_item(r)?);
        }
    }
    Ok(RetiredEquipmentReviewList { items, closed_count })
}

/// Occurrences of one watcher: [`NOTIFICATION_CATEGORY`] (open work orders on retired equipment)
/// or [`REQUEST_NOTIFICATION_CATEGORY`] (open requests). Derived from the synchronised state (not
/// from this PC's items), identified by [`ReviewKey::key`]. It began when both were true: the
/// later of the record's creation and the retirement.
pub(crate) async fn notice_candidates(
    conn: &impl ConnectionTrait,
    watcher: &str,
    severity: &str,
    fr: bool,
) -> AppResult<Vec<crate::notifications::watchers::Candidate>> {
    use crate::notifications::watchers::{parse_time, Candidate};
    let is_wo = watcher == NOTIFICATION_CATEGORY;
    let sql = if is_wo {
        open_work_orders_sql(&format!(
            "wo.id, wo.entity_sync_id AS sid, wo.code, wo.title, wo.created_at, \
             e.asset_id_code, e.name AS asset_name, {RETIRED_AT_SQL} AS retired_at, \
             (SELECT ul.code FROM reference_values ul WHERE ul.id = wo.urgency_id) AS urgency"
        )) + " ORDER BY wo.id"
    } else {
        open_requests_sql(&format!(
            "ir.id, ir.entity_sync_id AS sid, ir.code, ir.title, ir.created_at, \
             e.asset_id_code, e.name AS asset_name, {RETIRED_AT_SQL} AS retired_at, NULL AS urgency"
        )) + " ORDER BY ir.id"
    };
    let mut out = Vec::new();
    for r in conn.query_all(sqlite(&sql, vec![])).await? {
        let id = opt_i(&r, "id")?;
        let Some(sid) = opt_s(&r, "sid")? else { continue };
        let code = opt_s(&r, "code")?.unwrap_or_default();
        let title = opt_s(&r, "title")?.unwrap_or_default();
        let asset = opt_s(&r, "asset_id_code")?.unwrap_or_default();
        let asset_name = opt_s(&r, "asset_name")?.unwrap_or_default();
        let since = later(opt_s(&r, "created_at")?, opt_s(&r, "retired_at")?)
            .as_deref()
            .and_then(parse_time);
        let key = if is_wo {
            ReviewKey::work_order(sid)
        } else {
            ReviewKey::request(sid)
        };
        let (fr_title, en_title, fr_body, en_body) = if is_wo {
            (
                format!("OT {code} sur équipement déclassé — {asset}"),
                format!("Work order {code} on retired equipment — {asset}"),
                format!(
                    "{title} : OT ouvert alors que l'équipement {asset} ({asset_name}) est déclassé ou mis au rebut \
                     (croisement à la synchronisation). Annulez l'OT avec un motif, ou remettez l'équipement en service."
                ),
                format!(
                    "{title}: work order open while equipment {asset} ({asset_name}) is retired (crossed in \
                     synchronisation). Cancel the work order with a reason, or return the equipment to service."
                ),
            )
        } else {
            (
                format!("DI {code} sur équipement déclassé — {asset}"),
                format!("Request {code} on retired equipment — {asset}"),
                format!(
                    "{title} : demande ouverte alors que l'équipement {asset} ({asset_name}) est déclassé ou mis au \
                     rebut (croisement à la synchronisation). Rejetez la demande avec un motif, ou remettez l'équipement \
                     en service."
                ),
                format!(
                    "{title}: request open while equipment {asset} ({asset_name}) is retired (crossed in \
                     synchronisation). Reject the request with a reason, or return the equipment to service."
                ),
            )
        };
        out.push(Candidate {
            record_id: id,
            record_key: key.key(),
            since,
            source_module: if is_wo { "work_orders" } else { "di" },
            message_key: if is_wo {
                "retired_equipment_wo"
            } else {
                "retired_equipment_di"
            },
            params: serde_json::json!({ "code": code, "title": title, "asset": asset, "asset_name": asset_name }),
            urgency: opt_s(&r, "urgency")?,
            action_url: if is_wo {
                format!("/work-orders?openWo={}", id.unwrap_or_default())
            } else {
                format!("/requests?openDi={}", id.unwrap_or_default())
            },
            severity: severity.to_string(),
            title: if fr { fr_title } else { en_title },
            body: if fr { fr_body } else { en_body },
        });
    }
    Ok(out)
}

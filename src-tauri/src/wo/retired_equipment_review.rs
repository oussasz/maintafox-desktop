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
//! work order, request or equipment row (`sync::apply_hooks`), and [`reconcile_all_in`] runs before
//! the list is read (local actions are caught there).
//!
//! Closure. An item closes on any terminal state of its record, or when its equipment is no longer
//! retired, and keeps how ([`closure`]): the work order closed or cancelled, the request rejected
//! (`rejected_invalid`, `duplicate`), closed otherwise, or converted (the work order it became
//! carries its own item while open), or the equipment recommissioned. The time kept is the record's
//! own (its close, cancel, conversion time; the recommission event's time): the same on every PC.
//! Who closed it and their note are read from the synchronised transition log or lifecycle event
//! when the list is read, so a log that arrives after the record still names them.
//!
//! Settlement uses only the existing actions: cancel the work order or reject the request with a
//! reason, or reverse the retirement through the normal lifecycle right. There is no dismiss.
//!
//! Notice. The work order's planner, the request's reviewer and the responsibles of the record's
//! org node (the equipment's site) are told once per item, in-app (watcher
//! [`NOTIFICATION_CATEGORY`], `notifications::watchers`, audiences of its rule in Données de
//! référence), with the same identity on every PC.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

use crate::auth::scope::{scope_filter_sql, AccessScope};
use crate::errors::{AppError, AppResult};

/// Notification category (and watcher) of the work on retired equipment (migration 265).
pub const NOTIFICATION_CATEGORY: &str = "retired_equipment_work";

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
    /// When this PC found it.
    pub opened_at: String,
    pub closed_at: Option<String>,
    /// One of the `CLOSURE_*` codes.
    pub closure_kind: Option<String>,
    /// Who closed it (the transition or lifecycle event that ended it), when known here.
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
    /// Open items first (latest first), then the closed ones when asked for (latest closure first).
    pub items: Vec<RetiredEquipmentReview>,
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
}

async fn record_state(conn: &impl ConnectionTrait, key: &ReviewKey) -> AppResult<Option<RecordState>> {
    let retired = retired_sql();
    let row = if key.kind == KIND_WORK_ORDER {
        conn.query_one(sqlite(
            &format!(
                "SELECT wo.id, wo.equipment_id, s.code AS status, wo.closed_at, wo.cancelled_at, wo.updated_at, \
                        CASE WHEN {retired} THEN 1 ELSE 0 END AS retired, \
                        CASE WHEN e.id IS NOT NULL AND e.deleted_at IS NULL THEN 1 ELSE 0 END AS present \
                 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 LEFT JOIN equipment e ON e.id = wo.equipment_id WHERE wo.entity_sync_id = ?"
            ),
            vec![key.sync_id.clone().into()],
        ))
        .await?
    } else {
        conn.query_one(sqlite(
            &format!(
                "SELECT ir.id, ir.equipment_id, ir.status, ir.disposition_code, ir.closed_at, ir.declined_at, \
                        ir.converted_at, ir.updated_at, \
                        CASE WHEN {} THEN 1 ELSE 0 END AS is_open, \
                        CASE WHEN {retired} THEN 1 ELSE 0 END AS retired, \
                        CASE WHEN e.id IS NOT NULL AND e.deleted_at IS NULL THEN 1 ELSE 0 END AS present \
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
    let updated_at = opt_s(&row, "updated_at")?;
    let terminal = if key.kind == KIND_WORK_ORDER {
        match opt_s(&row, "status")?.as_deref() {
            Some("closed") => Some((CLOSURE_WO_CLOSED, opt_s(&row, "closed_at")?.or(updated_at))),
            Some("cancelled") => Some((CLOSURE_WO_CANCELLED, opt_s(&row, "cancelled_at")?.or(updated_at))),
            _ => None,
        }
    } else if opt_i(&row, "is_open")?.unwrap_or(0) == 1 {
        None
    } else {
        let status = opt_s(&row, "status")?.unwrap_or_default();
        let disposition = opt_s(&row, "disposition_code")?.unwrap_or_default();
        let converted_at = opt_s(&row, "converted_at")?;
        let closed_at = opt_s(&row, "closed_at")?;
        let declined_at = opt_s(&row, "declined_at")?;
        if converted_at.is_some()
            || disposition == crate::di::disposition::DISPOSITION_CONVERTED_TO_WO
            || status == "converted_to_work_order"
        {
            Some((CLOSURE_DI_CONVERTED, converted_at.or(closed_at).or(updated_at)))
        } else if REJECTED_DISPOSITIONS.contains(&disposition.as_str()) || status == "rejected" {
            Some((CLOSURE_DI_REJECTED, closed_at.or(declined_at).or(updated_at)))
        } else {
            Some((CLOSURE_DI_CLOSED, closed_at.or(updated_at)))
        }
    };
    Ok(Some(RecordState {
        record_id: row.try_get("", "id").map_err(|e| decode("id", e))?,
        equipment_id: opt_i(&row, "equipment_id")?,
        retired,
        in_service: present && !retired,
        terminal,
    }))
}

/// When the equipment was last returned to service (its latest `RECOMMISSIONED` event).
async fn recommissioned_at(conn: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Option<String>> {
    Ok(conn
        .query_one(sqlite(
            "SELECT occurred_at FROM equipment_lifecycle_events \
             WHERE equipment_id = ? AND event_type = 'RECOMMISSIONED' \
             ORDER BY occurred_at DESC, id DESC LIMIT 1",
            vec![equipment_id.into()],
        ))
        .await?
        .map(|r| opt_s(&r, "occurred_at"))
        .transpose()?
        .flatten())
}

/// How an open item closes, and when (the record's own time, the same on every PC): its record's
/// terminal state first, else its equipment returned to service, else [`CLOSURE_OTHER`] (now).
async fn closure(
    conn: &impl ConnectionTrait,
    state: Option<&RecordState>,
    item_equipment_id: i64,
) -> AppResult<(&'static str, String)> {
    if let Some(state) = state {
        if let Some((kind, at)) = &state.terminal {
            return Ok((*kind, at.clone().unwrap_or_else(now_iso)));
        }
        if state.in_service && state.equipment_id == Some(item_equipment_id) {
            let at = recommissioned_at(conn, item_equipment_id).await?;
            return Ok((CLOSURE_RECOMMISSIONED, at.unwrap_or_else(now_iso)));
        }
    }
    Ok((CLOSURE_OTHER, now_iso()))
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
            "SELECT id, equipment_id FROM retired_equipment_reviews \
             WHERE record_kind = ? AND record_sync_id = ? AND status = 'open'",
            vec![key.kind.into(), key.sync_id.clone().into()],
        ))
        .await?;
    match (open_now, item, state) {
        (true, None, Some(state)) => {
            // A reconcile running at the same time may have opened it: one open row per record.
            conn.execute(sqlite(
                "INSERT INTO retired_equipment_reviews \
                    (record_kind, record_sync_id, record_id, equipment_id, status, opened_at) \
                 VALUES (?, ?, ?, ?, 'open', ?) \
                 ON CONFLICT(record_kind, record_sync_id) WHERE status = 'open' DO NOTHING",
                vec![
                    key.kind.into(),
                    key.sync_id.clone().into(),
                    state.record_id.into(),
                    state.equipment_id.into(),
                    now_iso().into(),
                ],
            ))
            .await?;
        }
        (true, Some(item), Some(state)) => {
            // Still open: it follows the record's equipment (moved to another retired one).
            let id: i64 = item.try_get("", "id").map_err(|e| decode("id", e))?;
            conn.execute(sqlite(
                "UPDATE retired_equipment_reviews SET equipment_id = ?, record_id = ? \
                 WHERE id = ? AND status = 'open' AND (equipment_id IS NOT ? OR record_id IS NOT ?)",
                vec![
                    state.equipment_id.into(),
                    state.record_id.into(),
                    id.into(),
                    state.equipment_id.into(),
                    state.record_id.into(),
                ],
            ))
            .await?;
        }
        (false, Some(item), state) => {
            let id: i64 = item.try_get("", "id").map_err(|e| decode("id", e))?;
            let equipment_id: i64 = item
                .try_get("", "equipment_id")
                .map_err(|e| decode("equipment_id", e))?;
            let (kind, at) = closure(conn, state.as_ref(), equipment_id).await?;
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
        out.push(if kind == KIND_WORK_ORDER {
            ReviewKey::work_order(sid)
        } else {
            ReviewKey::request(sid)
        });
    }
    Ok(out)
}

/// Every work order and request of one equipment, and its open items, after the equipment row was
/// applied here (a retirement or a return to service received by sync).
pub async fn reconcile_equipment_in(conn: &impl ConnectionTrait, equipment_id: i64) -> AppResult<()> {
    let keys = keys_of(
        conn,
        &format!(
            "SELECT '{KIND_WORK_ORDER}' AS kind, entity_sync_id AS sid FROM work_orders WHERE equipment_id = ? \
             UNION SELECT '{KIND_REQUEST}', entity_sync_id FROM intervention_requests WHERE equipment_id = ? \
             UNION SELECT record_kind, record_sync_id FROM retired_equipment_reviews \
                   WHERE equipment_id = ? AND status = 'open'"
        ),
        vec![equipment_id.into(), equipment_id.into(), equipment_id.into()],
    )
    .await?;
    for key in keys {
        reconcile_in(conn, &key).await?;
    }
    Ok(())
}

/// [`reconcile_in`] for every open work order or request on retired equipment and every open item.
pub async fn reconcile_all_in(conn: &impl ConnectionTrait) -> AppResult<()> {
    let keys = keys_of(
        conn,
        &format!(
            "{} UNION {} UNION SELECT record_kind, record_sync_id FROM retired_equipment_reviews WHERE status = 'open'",
            open_work_orders_sql(&format!("'{KIND_WORK_ORDER}' AS kind, wo.entity_sync_id AS sid")),
            open_requests_sql(&format!("'{KIND_REQUEST}', ir.entity_sync_id")),
        ),
        vec![],
    )
    .await?;
    for key in keys {
        reconcile_in(conn, &key).await?;
    }
    Ok(())
}

const LIST_SELECT: &str = "SELECT r.id, r.record_kind, r.record_sync_id, r.equipment_id, r.status, r.opened_at, \
        r.closed_at, r.closure_kind, \
        CASE r.record_kind WHEN 'work_order' THEN wo.id ELSE ir.id END AS record_id, \
        CASE r.record_kind WHEN 'work_order' THEN wo.code ELSE ir.code END AS record_code, \
        CASE r.record_kind WHEN 'work_order' THEN wo.title ELSE ir.title END AS record_title, \
        CASE r.record_kind WHEN 'work_order' THEN s.code ELSE ir.status END AS record_status, \
        ir.disposition_code, ir.converted_to_wo_id, cwo.code AS converted_to_wo_code, \
        e.asset_id_code AS equipment_code, e.name AS equipment_name, e.lifecycle_status AS equipment_status \
     FROM retired_equipment_reviews r \
     LEFT JOIN work_orders wo ON r.record_kind = 'work_order' AND wo.entity_sync_id = r.record_sync_id \
     LEFT JOIN work_order_statuses s ON s.id = wo.status_id \
     LEFT JOIN intervention_requests ir ON r.record_kind = 'intervention_request' AND ir.entity_sync_id = r.record_sync_id \
     LEFT JOIN work_orders cwo ON cwo.id = ir.converted_to_wo_id \
     LEFT JOIN equipment e ON e.id = r.equipment_id";

/// Who ended the record and their note: the latest transition of the work order or request (for a
/// terminal record, the one that ended it), or the equipment's latest return to service.
async fn closed_by(
    conn: &impl ConnectionTrait,
    closure_kind: &str,
    record_id: Option<i64>,
    equipment_id: i64,
) -> AppResult<(Option<String>, Option<String>)> {
    let (sql, id) = match closure_kind {
        CLOSURE_WO_CLOSED | CLOSURE_WO_CANCELLED => (
            "SELECT ua.display_name AS who, l.notes AS note FROM wo_state_transition_log l \
             LEFT JOIN user_accounts ua ON ua.id = l.actor_id \
             WHERE l.wo_id = ? ORDER BY l.acted_at DESC, l.id DESC LIMIT 1",
            record_id,
        ),
        CLOSURE_DI_REJECTED | CLOSURE_DI_CLOSED | CLOSURE_DI_CONVERTED => (
            "SELECT ua.display_name AS who, l.notes AS note FROM di_state_transition_log l \
             LEFT JOIN user_accounts ua ON ua.id = l.actor_id \
             WHERE l.di_id = ? ORDER BY l.acted_at DESC, l.id DESC LIMIT 1",
            record_id,
        ),
        CLOSURE_RECOMMISSIONED => (
            "SELECT ua.display_name AS who, ev.notes AS note FROM equipment_lifecycle_events ev \
             LEFT JOIN user_accounts ua ON ua.id = ev.performed_by_id \
             WHERE ev.equipment_id = ? AND ev.event_type = 'RECOMMISSIONED' \
             ORDER BY ev.occurred_at DESC, ev.id DESC LIMIT 1",
            Some(equipment_id),
        ),
        _ => return Ok((None, None)),
    };
    let Some(id) = id else {
        return Ok((None, None));
    };
    match conn.query_one(sqlite(sql, vec![id.into()])).await? {
        Some(r) => Ok((opt_s(&r, "who")?, opt_s(&r, "note")?)),
        None => Ok((None, None)),
    }
}

/// The items the reader may see: work orders where they hold the work-order view right
/// (`wo_scope`, on the work order's org node), requests where they hold the request view right
/// (`di_scope`, on the request's node). The projection is brought up to date first.
pub async fn list_reviews(
    db: &DatabaseConnection,
    wo_scope: &AccessScope,
    di_scope: &AccessScope,
    include_closed: bool,
) -> AppResult<RetiredEquipmentReviewList> {
    let tx = db.begin().await?;
    reconcile_all_in(&tx).await?;
    tx.commit().await?;

    let (wo_pred, wo_binds) = scope_filter_sql(wo_scope, crate::wo::access::WO_ANCHOR_SQL);
    let (di_pred, di_binds) = scope_filter_sql(di_scope, crate::di::access::DI_ANCHOR_SQL);
    let status = if include_closed { "" } else { " AND r.status = 'open'" };
    let sql = format!(
        "{LIST_SELECT} WHERE ((r.record_kind = 'work_order' AND {wo_pred}) \
                        OR (r.record_kind = 'intervention_request' AND {di_pred})){status} \
         ORDER BY CASE r.status WHEN 'open' THEN 0 ELSE 1 END, \
                  COALESCE(r.closed_at, r.opened_at) DESC, r.id DESC"
    );
    let mut binds = wo_binds;
    binds.extend(di_binds);
    let rows = db.query_all(sqlite(&sql, binds)).await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let kind = opt_s(r, "record_kind")?.unwrap_or_default();
        let sync_id = opt_s(r, "record_sync_id")?.unwrap_or_default();
        let key = if kind == KIND_WORK_ORDER {
            ReviewKey::work_order(sync_id)
        } else {
            ReviewKey::request(sync_id)
        };
        let record_id = opt_i(r, "record_id")?;
        let equipment_id: i64 = r.try_get("", "equipment_id").map_err(|e| decode("equipment_id", e))?;
        let closure_kind = opt_s(r, "closure_kind")?;
        let (closed_by, closure_note) = match closure_kind.as_deref() {
            Some(k) => closed_by(db, k, record_id, equipment_id).await?,
            None => (None, None),
        };
        let is_request = kind == KIND_REQUEST;
        items.push(RetiredEquipmentReview {
            id: r.try_get("", "id").map_err(|e| decode("id", e))?,
            review_key: key.key(),
            kind,
            record_id,
            record_code: opt_s(r, "record_code")?,
            record_title: opt_s(r, "record_title")?,
            record_status: opt_s(r, "record_status")?,
            equipment_id,
            equipment_code: opt_s(r, "equipment_code")?,
            equipment_name: opt_s(r, "equipment_name")?,
            equipment_status: opt_s(r, "equipment_status")?,
            status: opt_s(r, "status")?.unwrap_or_default(),
            opened_at: opt_s(r, "opened_at")?.unwrap_or_default(),
            closed_at: opt_s(r, "closed_at")?,
            closure_kind,
            closed_by,
            closure_note,
            converted_to_wo_id: if is_request {
                opt_i(r, "converted_to_wo_id")?
            } else {
                None
            },
            converted_to_wo_code: if is_request {
                opt_s(r, "converted_to_wo_code")?
            } else {
                None
            },
            disposition_code: if is_request {
                opt_s(r, "disposition_code")?
            } else {
                None
            },
        });
    }
    Ok(RetiredEquipmentReviewList { items })
}

/// Occurrences of the watcher [`NOTIFICATION_CATEGORY`]: one per open work order or request on
/// retired equipment, derived from the synchronised state (not from this PC's items), identified by
/// [`ReviewKey::key`]. It began when both were true: the later of the record's creation and the
/// equipment's retirement.
pub(crate) async fn notice_candidates(
    conn: &impl ConnectionTrait,
    severity: &str,
    fr: bool,
) -> AppResult<Vec<crate::notifications::watchers::Candidate>> {
    use crate::notifications::watchers::{parse_time, Candidate};
    let mut out = Vec::new();
    let sources = [
        (
            KIND_WORK_ORDER,
            open_work_orders_sql(
                "wo.id, wo.entity_sync_id AS sid, wo.code, wo.title, wo.created_at, \
                 e.asset_id_code, e.name AS asset_name, e.decommissioned_at, \
                 (SELECT ul.code FROM reference_values ul WHERE ul.id = wo.urgency_id) AS urgency",
            ) + " ORDER BY wo.id",
        ),
        (
            KIND_REQUEST,
            open_requests_sql(
                "ir.id, ir.entity_sync_id AS sid, ir.code, ir.title, ir.created_at, \
                 e.asset_id_code, e.name AS asset_name, e.decommissioned_at, NULL AS urgency",
            ) + " ORDER BY ir.id",
        ),
    ];
    for (kind, sql) in sources {
        for r in conn.query_all(sqlite(&sql, vec![])).await? {
            let id = opt_i(&r, "id")?;
            let Some(sid) = opt_s(&r, "sid")? else { continue };
            let code = opt_s(&r, "code")?.unwrap_or_default();
            let title = opt_s(&r, "title")?.unwrap_or_default();
            let asset = opt_s(&r, "asset_id_code")?.unwrap_or_default();
            let asset_name = opt_s(&r, "asset_name")?.unwrap_or_default();
            let since = [opt_s(&r, "created_at")?, opt_s(&r, "decommissioned_at")?]
                .iter()
                .flatten()
                .filter_map(|t| parse_time(t))
                .max();
            let is_wo = kind == KIND_WORK_ORDER;
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
    }
    Ok(out)
}

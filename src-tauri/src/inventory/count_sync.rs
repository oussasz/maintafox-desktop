//! One stock count posted (or reversed) on two PCs (AU-100).
//!
//! A count movement and its audit link carry a seeded sync id (`controls::count_movement_sync_id`,
//! `controls::count_audit_link_sync_id`): the same on every PC for the same session, article and
//! location. The two PCs still wrote their own rows (who posted it, when, the reason, and — when
//! their lines differed — the quantity) for the movement, its audit link, the count session and its
//! line. One version of the posting is kept on every PC — the one the server received first:
//! - the second PC's push is answered with a server conflict; when the server row is the same count
//!   (movement, link: [`seeded_count_local`]) or the same posting (session, line: [`same_posting`])
//!   the outbox entry is closed here with no item for a person, and any conflict already opened for
//!   it is closed ([`settle_count_server_conflict`], called from `sync::queries`);
//! - a session or line received while this PC still has its own unsent posting of it is settled the
//!   same way ([`settle_concurrent_posting`], called from `sync::materialize::evaluate_and_apply`);
//! - the server's version then replaces this PC's rows: a session or line as an ordinary update, a
//!   movement or link through [`adopt_first_posted`] (`sync::row_codec` through `sync::apply_hooks`;
//!   append-only rows are otherwise never rewritten). A movement whose quantity differs from this
//!   PC's moves this PC's stock by the difference in the same transaction, so every PC holds the
//!   same ledger and the same stock.
//!
//! A count reversed on this PC before its posting is settled with the server (posted and reversed
//! offline) waits in the outbox ([`REVERSAL_AWAITING_ITS_POSTING_SQL`]): it is sent once the
//! posting is acknowledged, or once the version posted first on another PC is applied here. When
//! that version has another quantity, the reversal is first made the exact undo of it and this PC's
//! stock follows ([`undo_kept_posting`]): the other PCs receive the reversal of the posting they
//! hold and every PC goes back to the same stock (review of ab58cd53, §4). Sent before the posting
//! was settled, it would undo a posting no other PC holds and be refused there.
//!
//! This PC's posting (who, when, the PC, its reason and quantity) is kept in an audit event
//! [`AUDIT_POSTED_TWICE`] for the session, the movement and the audit link; the event is
//! synchronised. Rows that are identical on both PCs need nothing: the server acknowledges the second
//! push as a duplicate.
//!
//! What stays for a person: a count line that differs (counted quantity, variance, reason, approval)
//! remains a sync conflict on the PC whose push lost — that PC holds the line and the person who
//! resolves it. The movement and its audit link are settled even then (they carry the variance
//! reason and the reviewer's evidence copied from the line: the kept version keeps the first PC's;
//! the audit says `line_differs`). A count whose quantity differed is listed "comptage divergent —
//! à vérifier" (`negative_stock`), from its audit event: on that PC first, then on every PC the event
//! reaches; the stock itself is already the same everywhere. It is closed by a new count.
//!
//! A count posted twice by older versions (two random ids) is applied like any movement — the
//! ledger stays the same on every PC — and is listed "comptage passé deux fois — à vérifier"
//! (`negative_stock::DoublePostedCountReview`).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{Map, Value};

use crate::errors::AppResult;
use crate::inventory::controls::{count_audit_link_sync_id, count_movement_kind, COUNT_ALREADY_POSTED};
use crate::sync::row_codec::{load_row_payload, RowLocator};

const MOVEMENTS: &str = "inventory_transactions";
const AUDIT_LINKS: &str = "inventory_mutation_audit_links";
const SESSIONS: &str = "inventory_count_sessions";
const LINES: &str = "inventory_count_lines";

/// Session fields that only say who posted or reversed it, when, and the reversal reason each
/// person typed: two versions differing only there are the same posting.
const SESSION_ATTRIBUTION: &[&str] = &[
    "posted_by_sync_id",
    "posted_at",
    "reversed_by_sync_id",
    "reversed_at",
    "reversal_reason",
    "updated_at",
];
/// Line fields that only date the change.
const LINE_ATTRIBUTION: &[&str] = &["updated_at"];
/// Movement fields copied from the count line (a difference there is also a line difference).
const MOVEMENT_LINE_FIELDS: &[&str] = &["reason", "quantity"];
/// Audit-link fields copied from the count line.
const LINK_LINE_FIELDS: &[&str] = &["reviewer_sync_id", "reviewer_evidence"];

/// Audit action of a count posting superseded by the same posting made first on another PC.
pub const AUDIT_POSTED_TWICE: &str = "inventory.count.posted_twice";

/// SQL condition on an outbox entry (alias `o`): it is a count reversal (`COUNT_REVERSAL` of a
/// count session) whose count's adjustment, posted on this PC, is not settled with the server yet.
/// That adjustment's push is unsent, in flight or in conflict; or it is dead-lettered (its delivery
/// failed too often: the server may have received it and answered that the same count was posted
/// first elsewhere, piece F review SF-2) or superseded by the same count posted first on another PC
/// ([`COUNT_ALREADY_POSTED`]), and in both cases the server's version is not applied here yet. The
/// reversal
/// waits in the outbox (`sync::queries::get_sync_push_payload`): when the posting kept has another
/// quantity, the reversal is made the exact undo of that one before it is sent
/// ([`adopt_first_posted`]), so every PC goes back to the same stock (review of ab58cd53, §4). An
/// adjustment the server rejected, or acknowledged, holds nothing.
pub(crate) const REVERSAL_AWAITING_ITS_POSTING_SQL: &str = "EXISTS (
    SELECT 1 FROM inventory_transactions r
    JOIN inventory_transactions x
      ON x.movement_type = 'COUNT_ADJUST' AND x.source_type = 'COUNT_SESSION' AND x.source_id = r.source_id
     AND x.article_id = r.article_id AND x.location_id = r.location_id
    WHERE o.entity_type = 'inventory_transactions' AND r.entity_sync_id = o.entity_sync_id
      AND r.movement_type = 'COUNT_REVERSAL' AND r.source_type = 'COUNT_SESSION'
      AND (EXISTS (SELECT 1 FROM sync_outbox xo
                    WHERE xo.entity_type = 'inventory_transactions' AND xo.entity_sync_id = x.entity_sync_id
                      AND xo.status IN ('pending', 'in_flight', 'conflict'))
           OR (EXISTS (SELECT 1 FROM sync_outbox xo
                        WHERE xo.entity_type = 'inventory_transactions' AND xo.entity_sync_id = x.entity_sync_id
                          AND (xo.status = 'dead_letter'
                               OR (xo.status = 'superseded' AND xo.rejection_code = 'COUNT_ALREADY_POSTED')))
               AND NOT EXISTS (SELECT 1 FROM sync_entity_state s
                                WHERE s.entity_type = 'inventory_transactions'
                                  AND s.entity_sync_id = x.entity_sync_id))))";

/// Quantities closer than this are equal (decimal movements).
const QTY_TOLERANCE: f64 = 1e-9;

fn text<'a>(payload: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
}

fn qty(payload: &Map<String, Value>) -> Option<f64> {
    payload.get("quantity").and_then(Value::as_f64)
}

/// Two wire values mean the same: a missing key is null; numbers compare as numbers (`-3` and
/// `-3.0`), within [`QTY_TOLERANCE`].
fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    let a = a.filter(|v| !v.is_null());
    let b = b.filter(|v| !v.is_null());
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= QTY_TOLERANCE,
            _ => x == y,
        },
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Keys whose meaning differs between two wire versions of a row ([`same_value`]), sorted. A key
/// only one version carries (a column the other PC's version does not have yet) is not compared.
fn differing_keys<'a>(a: &'a Map<String, Value>, b: &'a Map<String, Value>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = a
        .keys()
        .chain(b.keys())
        .map(String::as_str)
        .filter(|k| a.contains_key(*k) && b.contains_key(*k) && !same_value(a.get(*k), b.get(*k)))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// This PC's row of an entity, as the wire carries it (references by sync id).
async fn local_payload(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<Map<String, Value>>> {
    let layout = crate::sync::row_layout::layout(conn, entity_type).await?;
    Ok(
        load_row_payload(conn, &layout, &RowLocator::Identity(entity_sync_id.to_string()))
            .await?
            .map(|p| p.payload),
    )
}

/// `entity_sync_id` is the seeded id of the count movement `payload` describes.
pub(crate) fn is_seeded_count_movement(entity_sync_id: &str, payload: &Map<String, Value>) -> bool {
    let Some(kind) = text(payload, "movement_type").and_then(count_movement_kind) else {
        return false;
    };
    if text(payload, "source_type") != Some("COUNT_SESSION") {
        return false;
    }
    let (Some(session), Some(article), Some(location)) = (
        text(payload, "source_sync_id"),
        text(payload, "article_sync_id"),
        text(payload, "location_sync_id"),
    ) else {
        return false;
    };
    crate::sync::identity::seeded_entity_sync_id(kind, &format!("{session}:{article}:{location}")) == entity_sync_id
}

fn same_texts(a: &Map<String, Value>, b: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().all(|k| text(a, k).is_some() && text(a, k) == text(b, k))
}

/// This PC's row (wire form) when `entity_sync_id` is a seeded count id held here and `other` (the
/// server's or a received version of that row) is the same count: same movement type, session,
/// article and location for a movement (its quantity may differ when the two PCs' lines differed:
/// the first pushed is kept and the stock follows it, [`adopt_first_posted`]); the same movement and
/// count line for its audit link. `None` otherwise.
pub(crate) async fn seeded_count_local(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    other: &Map<String, Value>,
) -> AppResult<Option<Map<String, Value>>> {
    match entity_type {
        MOVEMENTS => {
            let Some(local) = local_payload(conn, MOVEMENTS, entity_sync_id).await? else {
                return Ok(None);
            };
            if !is_seeded_count_movement(entity_sync_id, &local) || qty(other).is_none() {
                return Ok(None);
            }
            let same = same_texts(
                &local,
                other,
                &[
                    "movement_type",
                    "source_type",
                    "source_sync_id",
                    "article_sync_id",
                    "location_sync_id",
                ],
            );
            Ok(same.then_some(local))
        }
        AUDIT_LINKS => {
            let Some(local) = local_payload(conn, AUDIT_LINKS, entity_sync_id).await? else {
                return Ok(None);
            };
            let Some(movement) = text(&local, "transaction_sync_id") else {
                return Ok(None);
            };
            if count_audit_link_sync_id(movement) != entity_sync_id {
                return Ok(None);
            }
            let Some(movement_row) = local_payload(conn, MOVEMENTS, movement).await? else {
                return Ok(None);
            };
            if !is_seeded_count_movement(movement, &movement_row) {
                return Ok(None);
            }
            let same = same_texts(
                &local,
                other,
                &["transaction_sync_id", "source_entity_type", "source_entity_sync_id"],
            );
            Ok(same.then_some(local))
        }
        _ => Ok(None),
    }
}

/// This PC's count session or count line (wire form) when `other` (the server's or a received
/// version) is the same posting of it: a session both `posted` (or both `reversed`) that differs
/// only by who posted or reversed it, when, and the reversal reason ([`SESSION_ATTRIBUTION`]); a
/// line that differs only by its update time, whose posted (and reversed) movement is the seeded
/// movement of that count. `None` for any other difference: it stays a conflict for a person.
pub(crate) async fn same_posting(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    other: &Map<String, Value>,
) -> AppResult<Option<Map<String, Value>>> {
    let allowed = match entity_type {
        SESSIONS => SESSION_ATTRIBUTION,
        LINES => LINE_ATTRIBUTION,
        _ => return Ok(None),
    };
    let Some(local) = local_payload(conn, entity_type, entity_sync_id).await? else {
        return Ok(None);
    };
    if entity_type == SESSIONS {
        let status = text(&local, "status");
        if !matches!(status, Some("posted" | "reversed")) || status != text(other, "status") {
            return Ok(None);
        }
    } else {
        let (Some(session), Some(article), Some(location)) = (
            text(&local, "session_sync_id"),
            text(&local, "article_sync_id"),
            text(&local, "location_sync_id"),
        ) else {
            return Ok(None);
        };
        let seeded =
            |kind: &str| crate::sync::identity::seeded_entity_sync_id(kind, &format!("{session}:{article}:{location}"));
        if text(&local, "posted_transaction_sync_id") != Some(seeded("count_adjust").as_str()) {
            return Ok(None);
        }
        if let Some(reversed) = text(&local, "reversed_transaction_sync_id") {
            if reversed != seeded("count_reversal") {
                return Ok(None);
            }
        }
    }
    let same = differing_keys(&local, other).iter().all(|k| allowed.contains(k));
    Ok(same.then_some(local))
}

/// Which outbox entries [`close_outbox`] closes.
enum OutboxScope<'a> {
    /// The entry the server answered.
    One(i64),
    /// Every unsent change of a row (`entity type`, `sync id`).
    Unsent(&'a str, &'a str),
}

/// Closes outbox entries (`superseded`, coded [`COUNT_ALREADY_POSTED`]) and the conflicts still
/// open for them (`resolved_remote`): the version kept is the one the server received first.
async fn close_outbox(conn: &impl ConnectionTrait, scope: OutboxScope<'_>, message: String) -> AppResult<()> {
    let (filter, values): (&str, Vec<sea_orm::Value>) = match scope {
        OutboxScope::One(id) => ("id = ?", vec![id.into()]),
        OutboxScope::Unsent(entity_type, entity_sync_id) => (
            "entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')",
            vec![entity_type.into(), entity_sync_id.into()],
        ),
    };
    let mut conflict_values: Vec<sea_orm::Value> = vec![message.clone().into()];
    conflict_values.extend(values.iter().cloned());
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE sync_conflicts
             SET status = 'resolved_remote', resolution_action = 'same_count_kept', resolution_note = ?,
                 resolved_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE status IN ('new', 'triaged', 'escalated')
               AND linked_outbox_id IN (SELECT id FROM sync_outbox WHERE {filter})"
        ),
        conflict_values,
    ))
    .await?;
    let mut outbox_values: Vec<sea_orm::Value> = vec![COUNT_ALREADY_POSTED.into(), message.into()];
    outbox_values.extend(values);
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE sync_outbox
             SET status = 'superseded', rejection_code = ?, rejection_message = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE {filter}"
        ),
        outbox_values,
    ))
    .await?;
    Ok(())
}

/// A server conflict on a row of a count pushed by this PC — its movement, audit link, session or
/// line — when the server holds the same count posted first on another PC ([`seeded_count_local`],
/// [`same_posting`]): the outbox entry is closed (`superseded`, coded [`COUNT_ALREADY_POSTED`]) and
/// no conflict is recorded. The server's version is applied here when it arrives (first push wins).
/// A session's own posting is audited now, before it is replaced (when it differs at all).
/// `true` when settled.
pub(crate) async fn settle_count_server_conflict(
    conn: &impl ConnectionTrait,
    conflict: &crate::sync::domain::SyncServerConflictInput,
    linked_outbox_id: Option<i64>,
) -> AppResult<bool> {
    if conflict.server_deleted {
        return Ok(false);
    }
    let Some(server) = conflict
        .server_payload_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Map<String, Value>>(json).ok())
    else {
        return Ok(false);
    };
    let (entity_type, entity_sync_id) = (conflict.entity_type.as_str(), conflict.entity_sync_id.as_str());
    match entity_type {
        MOVEMENTS | AUDIT_LINKS => {
            if seeded_count_local(conn, entity_type, entity_sync_id, &server)
                .await?
                .is_none()
            {
                return Ok(false);
            }
        }
        SESSIONS | LINES => {
            let Some(local) = same_posting(conn, entity_type, entity_sync_id, &server).await? else {
                return Ok(false);
            };
            if entity_type == SESSIONS && !differing_keys(&local, &server).is_empty() {
                write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, &server, None).await?;
            }
        }
        _ => return Ok(false),
    }
    if let Some(outbox_id) = linked_outbox_id {
        close_outbox(
            conn,
            OutboxScope::One(outbox_id),
            format!(
                "The same count was posted first on another PC (server version {}): its version is kept here.",
                conflict.server_row_version
            ),
        )
        .await?;
    }
    Ok(true)
}

/// A received count session or line while this PC still has an unsent change of it
/// (`sync::materialize::evaluate_and_apply`, before its concurrency check): when it is the same
/// posting ([`same_posting`]) and differs at all, this PC's unsent change is closed (`superseded`)
/// and the received version is then applied (it reached the server first); a session's own posting
/// is audited first. A received row identical to this PC's (its own change sent back) or not newer
/// than the version applied here is left to the usual checks. `true` when settled.
pub(crate) async fn settle_concurrent_posting(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    row_version: i64,
    payload_json: &str,
) -> AppResult<bool> {
    if entity_type != SESSIONS && entity_type != LINES {
        return Ok(false);
    }
    // A received version not newer than the one applied here never closes this PC's change: it is
    // left to the usual checks (a `CONCURRENT_EDIT` for a person while this change is unsent).
    if let Some((synced, _)) = crate::sync::queries::synced_row_version(conn, entity_type, entity_sync_id).await? {
        if row_version <= synced {
            return Ok(false);
        }
    }
    let unsent = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS unsent FROM sync_outbox
             WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')
             LIMIT 1",
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?;
    if unsent.is_none() {
        return Ok(false);
    }
    let Ok(received) = serde_json::from_str::<Map<String, Value>>(payload_json) else {
        return Ok(false);
    };
    let Some(local) = same_posting(conn, entity_type, entity_sync_id, &received).await? else {
        return Ok(false);
    };
    if differing_keys(&local, &received).is_empty() {
        return Ok(false);
    }
    if entity_type == SESSIONS {
        write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, &received, None).await?;
    }
    close_outbox(
        conn,
        OutboxScope::Unsent(entity_type, entity_sync_id),
        "The same count was posted on another PC and reached the server first: its version is kept here.".to_string(),
    )
    .await?;
    Ok(true)
}

/// Name of a user known by sync id, when the account is here.
async fn user_name(conn: &impl ConnectionTrait, user_sync_id: Option<&str>) -> AppResult<Option<String>> {
    let Some(user_sync_id) = user_sync_id else {
        return Ok(None);
    };
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(NULLIF(trim(display_name), ''), username) AS name FROM user_accounts WHERE entity_sync_id = ?",
            [user_sync_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<String>("", "name")).transpose()?)
}

/// What the audit keeps of one version of the posting.
async fn version_details(conn: &impl ConnectionTrait, entity_type: &str, p: &Map<String, Value>) -> AppResult<Value> {
    Ok(match entity_type {
        MOVEMENTS => serde_json::json!({
            "posted_by": user_name(conn, text(p, "performed_by_sync_id")).await?,
            "posted_by_sync_id": text(p, "performed_by_sync_id"),
            "posted_at": text(p, "performed_at"),
            "reason": text(p, "reason"),
            "quantity": qty(p),
        }),
        SESSIONS => serde_json::json!({
            "status": text(p, "status"),
            "posted_by": user_name(conn, text(p, "posted_by_sync_id")).await?,
            "posted_by_sync_id": text(p, "posted_by_sync_id"),
            "posted_at": text(p, "posted_at"),
            "reversed_by": user_name(conn, text(p, "reversed_by_sync_id")).await?,
            "reversed_by_sync_id": text(p, "reversed_by_sync_id"),
            "reversed_at": text(p, "reversed_at"),
            "reversal_reason": text(p, "reversal_reason"),
        }),
        _ => serde_json::json!({
            "reviewer": user_name(conn, text(p, "reviewer_sync_id")).await?,
            "reviewer_sync_id": text(p, "reviewer_sync_id"),
            "reviewer_evidence": text(p, "reviewer_evidence"),
            "created_at": text(p, "created_at"),
        }),
    })
}

/// Audit event [`AUDIT_POSTED_TWICE`] for one row of a count posted on two PCs: this PC's version
/// (who posted or reversed it, when, its reason and quantity), the version kept, this PC, and
/// whether the count lines differed (`line_differs`) or the quantity did (`quantity_differs`, read
/// by `negative_stock` to list the count as divergent), and this PC's reversal of it made the undo
/// of the posting kept (`reversal_undoes_kept`, [`undo_kept_posting`]). Returns the event's id.
async fn write_posted_twice_audit(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    local: &Map<String, Value>,
    kept: &Map<String, Value>,
    reversal: Option<&Value>,
) -> AppResult<i64> {
    let differing = differing_keys(local, kept);
    let line_fields: &[&str] = match entity_type {
        MOVEMENTS => MOVEMENT_LINE_FIELDS,
        AUDIT_LINKS => LINK_LINE_FIELDS,
        _ => &[],
    };
    let line_differs = differing.iter().any(|k| line_fields.contains(k));
    let quantity_differs = entity_type == MOVEMENTS && differing.contains(&"quantity");
    let replica_id = crate::sync::queries::local_replica_id(conn).await?;
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_POSTED_TWICE.to_string(),
            target_type: Some(entity_type.to_string()),
            target_id: Some(entity_sync_id.to_string()),
            actor_id: None,
            auth_context: "system".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "The same stock count was posted on two PCs: the version received first by the server is kept on every PC and this PC's posting is recorded here",
                "count_session_sync_id": match entity_type {
                    SESSIONS => Some(entity_sync_id),
                    MOVEMENTS => text(local, "source_sync_id"),
                    _ => None,
                },
                "movement_sync_id": match entity_type {
                    MOVEMENTS => Some(entity_sync_id),
                    AUDIT_LINKS => text(local, "transaction_sync_id"),
                    _ => None,
                },
                "movement_type": text(local, "movement_type"),
                "line_differs": line_differs,
                "quantity_differs": quantity_differs,
                "device_code": crate::sync::device_code::device_code_for(&replica_id),
                "replica_id": replica_id,
                "this_pc_posting": version_details(conn, entity_type, local).await?,
                "kept_posting": version_details(conn, entity_type, kept).await?,
                "reversal_undoes_kept": reversal,
            })),
        },
    )
    .await
}

/// How much adopting the received version `inbound` of a count movement over the row held here
/// (`local`, wire form) moves this PC's stock ([`adopt_first_posted`]): the difference of their
/// quantities, `None` when they agree (or one is not given).
pub(crate) fn adoption_delta(local: &Map<String, Value>, inbound: &Map<String, Value>) -> Option<f64> {
    let delta = qty(inbound)? - qty(local)?;
    (delta.abs() > QTY_TOLERANCE).then_some(delta)
}

/// A received count movement or audit link that this PC already holds under the same seeded id,
/// for the same count ([`seeded_count_local`]), with content that differs in meaning (this PC posted
/// the same count too and its push lost): `true` tells the caller to write the received content over
/// this PC's row, so the version the server received first is the one every PC holds.
///
/// A movement whose quantity differs (the two PCs' count lines differed) moves this PC's stock by
/// the difference here, in the same transaction, and the location's negative-stock item is brought
/// in line: every PC then holds the same ledger and the same stock. This PC's own posting is audited
/// ([`AUDIT_POSTED_TWICE`]) and the event is queued for synchronisation (this apply runs with capture
/// suppressed). Only this PC's own posting, never synchronised, is replaced so: a received version
/// changing the quantity of a count movement received or acknowledged here is refused before it
/// gets here (`count_four_eyes::inbound_count_movement_hold`), and an error here if it ever does
/// (nothing is written: the received row is a materialize error for a person).
pub(crate) async fn adopt_first_posted(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    inbound: &Map<String, Value>,
) -> AppResult<bool> {
    let Some(local) = seeded_count_local(conn, entity_type, entity_sync_id, inbound).await? else {
        return Ok(false);
    };
    if differing_keys(&local, inbound).is_empty() {
        return Ok(false);
    }
    let mut reversal = None;
    if entity_type == MOVEMENTS {
        let (Some(_), Some(kept_qty)) = (qty(&local), qty(inbound)) else {
            return Ok(false);
        };
        if let Some(delta) = adoption_delta(&local, inbound) {
            // Only this PC's own posting, never synchronised, moves the stock so; checked here too,
            // not only by the hold that runs before (review of ab58cd53, N3): a movement received or
            // acknowledged here is never moved by a later version of it.
            if crate::sync::queries::synced_row_version(conn, MOVEMENTS, entity_sync_id)
                .await?
                .is_some()
            {
                return Err(crate::errors::AppError::SyncError(format!(
                    "count movement {entity_sync_id} is synchronised here: a received version changing its quantity \
                     is never adopted (it would move the stock with no count line behind it)"
                )));
            }
            let row = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT article_id, warehouse_id, location_id, movement_type FROM inventory_transactions
                     WHERE entity_sync_id = ?",
                    [entity_sync_id.into()],
                ))
                .await?
                .ok_or_else(|| {
                    crate::errors::AppError::SyncError(format!("count movement {entity_sync_id} vanished"))
                })?;
            let (article_id, location_id): (i64, i64) =
                (row.try_get("", "article_id")?, row.try_get("", "location_id")?);
            let movement_type: String = row.try_get("", "movement_type")?;
            crate::inventory::ledger::apply_received_movement(
                conn,
                article_id,
                row.try_get("", "warehouse_id")?,
                location_id,
                &movement_type,
                delta,
            )
            .await?;
            // The kept quantity is written before the location is brought in line, so a review item
            // opened now names its movement from the ledger as every PC holds it (the caller then
            // writes the rest of the kept content).
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inventory_transactions SET quantity = ? WHERE entity_sync_id = ?",
                [kept_qty.into(), entity_sync_id.into()],
            ))
            .await?;
            if movement_type == "COUNT_ADJUST" {
                reversal = undo_kept_posting(conn, &local, kept_qty).await?;
            }
            crate::inventory::negative_stock::reconcile_pair_in(conn, article_id, location_id).await?;
        }
    }
    let audit_id =
        write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, inbound, reversal.as_ref()).await?;
    crate::sync::capture::capture_row_change(conn, "audit_events", audit_id).await?;
    Ok(true)
}

/// This PC's reversal of its own posting of a count, still waiting in the outbox for that posting
/// to be settled with the server ([`REVERSAL_AWAITING_ITS_POSTING_SQL`]), when the posting kept (the
/// one the server received first, `kept_qty`) has another quantity than this PC's (`local`, wire
/// form): the reversal is made the exact undo of the posting kept (`-kept_qty`) and this PC's
/// stock follows, in the adoption's transaction (review of ab58cd53, §4). Its unsent outbox entry
/// carries the new quantity, so every PC receives the reversal of the posting it holds and goes
/// back to the same stock; with the adoption the stock here does not move at all (posted, then
/// reversed). Only a reversal never sent (no version of it synchronised, no outbox entry of it
/// beyond `pending`, nor one ever attempted: a delivery that failed may have reached the server,
/// piece F review SF-1) that undoes exactly this PC's posting. Returns what the audit keeps of it.
async fn undo_kept_posting(
    conn: &impl ConnectionTrait,
    local: &Map<String, Value>,
    kept_qty: f64,
) -> AppResult<Option<Value>> {
    let (Some(session), Some(article), Some(location), Some(posted)) = (
        text(local, "source_sync_id"),
        text(local, "article_sync_id"),
        text(local, "location_sync_id"),
        qty(local),
    ) else {
        return Ok(None);
    };
    let reversal_sync_id =
        crate::sync::identity::seeded_entity_sync_id("count_reversal", &format!("{session}:{article}:{location}"));
    let Some(reversal) = local_payload(conn, MOVEMENTS, &reversal_sync_id).await? else {
        return Ok(None);
    };
    let Some(undone) = qty(&reversal) else {
        return Ok(None);
    };
    if text(&reversal, "movement_type") != Some("COUNT_REVERSAL")
        || !is_seeded_count_movement(&reversal_sync_id, &reversal)
        || (undone + posted).abs() > QTY_TOLERANCE
    {
        return Ok(None);
    }
    if crate::sync::queries::synced_row_version(conn, MOVEMENTS, &reversal_sync_id)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let sent = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS sent FROM sync_outbox WHERE entity_type = ? AND entity_sync_id = ?
               AND (status <> 'pending' OR attempt_count > 0 OR last_attempt_at IS NOT NULL)
             LIMIT 1",
            [MOVEMENTS.into(), reversal_sync_id.clone().into()],
        ))
        .await?;
    if sent.is_some() {
        return Ok(None);
    }
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_id, warehouse_id, location_id FROM inventory_transactions WHERE entity_sync_id = ?",
            [reversal_sync_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| crate::errors::AppError::SyncError(format!("count reversal {reversal_sync_id} vanished")))?;
    let undo = -kept_qty;
    crate::inventory::ledger::apply_received_movement(
        conn,
        row.try_get("", "article_id")?,
        row.try_get("", "warehouse_id")?,
        row.try_get("", "location_id")?,
        "COUNT_REVERSAL",
        undo - undone,
    )
    .await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inventory_transactions SET quantity = ? WHERE entity_sync_id = ?",
        [undo.into(), reversal_sync_id.clone().into()],
    ))
    .await?;
    // Its unsent outbox entry (staged before the adoption, with this PC's undo) is rewritten with
    // the row as it now is: the reversal has never left this PC, so no version of it is replaced
    // anywhere else. Not staged yet (still in the capture journal), it is staged as it now is.
    let layout = crate::sync::row_layout::layout(conn, MOVEMENTS).await?;
    if let Some(row) = load_row_payload(conn, &layout, &RowLocator::Identity(reversal_sync_id.clone())).await? {
        let payload_json = row.payload_json()?;
        let payload_hash = crate::sync::queries::hash_payload(&payload_json);
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sync_outbox SET payload_json = ?, payload_hash = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE entity_type = ? AND entity_sync_id = ? AND status = 'pending'",
            [
                payload_json.into(),
                payload_hash.into(),
                MOVEMENTS.into(),
                reversal_sync_id.clone().into(),
            ],
        ))
        .await?;
    }
    Ok(Some(serde_json::json!({
        "movement_sync_id": reversal_sync_id,
        "this_pc_quantity": undone,
        "sent_quantity": undo,
    })))
}

//! One stock count posted (or reversed) on two PCs (AU-100).
//!
//! A count movement and its audit link carry a seeded sync id (`controls::count_movement_sync_id`,
//! `controls::count_audit_link_sync_id`): the same on every PC for the same session, article and
//! location, so the stock moves once. The two PCs still wrote their own rows (who posted it, when,
//! the reversal reason they typed) for the movement, its audit link, the count session and its
//! line. One version of the posting is kept on every PC — the one the server received first:
//! - the second PC's push is answered with a server conflict; when the server row is the same count
//!   (movement, link: [`same_seeded_count`]) or the same posting (session, line: [`same_posting`])
//!   the outbox entry is closed here with no item for a person ([`settle_count_server_conflict`],
//!   called from `sync::queries`);
//! - a session or line received while this PC still has its own unsent posting of it is settled the
//!   same way ([`settle_concurrent_posting`], called from `sync::materialize`);
//! - the server's version then replaces this PC's rows: a session or line as an ordinary update, a
//!   movement or link through [`adopt_first_posted`] (`sync::row_codec` through `sync::apply_hooks`;
//!   append-only rows are otherwise never rewritten). The stock is not touched: the movement was
//!   already applied here once.
//!
//! This PC's posting (who, when, the PC, its reason) is kept in an audit event
//! [`AUDIT_POSTED_TWICE`] for the session, the movement and the audit link. Any other difference
//! stays a conflict for a person. A count posted twice by older versions (two random ids) is applied
//! like any movement — the ledger stays the same on every PC — and is listed "comptage passé deux
//! fois — à vérifier" (`negative_stock::DoublePostedCountReview`).

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

/// Audit action of a count posting superseded by the same posting made first on another PC.
pub const AUDIT_POSTED_TWICE: &str = "inventory.count.posted_twice";

/// Quantities closer than this are equal (decimal movements).
const QTY_TOLERANCE: f64 = 1e-9;

fn text<'a>(payload: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
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
fn is_seeded_count_movement(entity_sync_id: &str, payload: &Map<String, Value>) -> bool {
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

/// Keys whose values differ between two wire versions of a row.
fn differing_keys<'a>(a: &'a Map<String, Value>, b: &'a Map<String, Value>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = a
        .keys()
        .chain(b.keys())
        .map(String::as_str)
        .filter(|k| a.get(*k) != b.get(*k))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// This PC's row (wire form) when `entity_sync_id` is a seeded count id held here and `other` (the
/// server's or a received version of that row) is the same count: same movement type, session,
/// article, location and quantity for a movement; the same movement and count line for its audit
/// link. `None` otherwise.
pub(crate) async fn same_seeded_count(
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
            if !is_seeded_count_movement(entity_sync_id, &local) {
                return Ok(None);
            }
            let same_keys = same_texts(
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
            let qty = |p: &Map<String, Value>| p.get("quantity").and_then(Value::as_f64);
            let same_qty = matches!((qty(&local), qty(other)), (Some(a), Some(b)) if (a - b).abs() <= QTY_TOLERANCE);
            Ok((same_keys && same_qty).then_some(local))
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

/// Closes outbox entries (`superseded`, coded [`COUNT_ALREADY_POSTED`]).
async fn close_outbox(conn: &impl ConnectionTrait, scope: OutboxScope<'_>, message: String) -> AppResult<()> {
    let (filter, mut values): (&str, Vec<sea_orm::Value>) = match scope {
        OutboxScope::One(id) => ("id = ?", vec![id.into()]),
        OutboxScope::Unsent(entity_type, entity_sync_id) => (
            "entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')",
            vec![entity_type.into(), entity_sync_id.into()],
        ),
    };
    let mut all: Vec<sea_orm::Value> = vec![COUNT_ALREADY_POSTED.into(), message.into()];
    all.append(&mut values);
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &format!(
            "UPDATE sync_outbox
             SET status = 'superseded', rejection_code = ?, rejection_message = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE {filter}"
        ),
        all,
    ))
    .await?;
    Ok(())
}

/// A server conflict on a row of a count pushed by this PC — its movement, audit link, session or
/// line — when the server holds the same count posted first on another PC ([`same_seeded_count`],
/// [`same_posting`]): the outbox entry is closed (`superseded`, coded [`COUNT_ALREADY_POSTED`]) and
/// no conflict is recorded. The server's version is applied here when it arrives (first push wins).
/// A session's own posting is audited now, before it is replaced. `true` when settled.
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
            if same_seeded_count(conn, entity_type, entity_sync_id, &server)
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
            if entity_type == SESSIONS {
                write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, &server).await?;
            }
        }
        _ => return Ok(false),
    }
    if let Some(outbox_id) = linked_outbox_id {
        close_outbox(
            conn,
            OutboxScope::One(outbox_id),
            format!(
                "The same count was posted first on another PC (server version {}): the stock moved once; the server's version is kept.",
                conflict.server_row_version
            ),
        )
        .await?;
    }
    Ok(true)
}

/// A received count session or line while this PC still has an unsent change of it
/// (`sync::materialize`, instead of a `CONCURRENT_EDIT`): when it is the same posting
/// ([`same_posting`]) this PC's change is closed (`superseded`) and the received version is applied
/// (it reached the server first); a session's own posting is audited first. `true` when settled.
pub(crate) async fn settle_concurrent_posting(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    payload_json: &str,
) -> AppResult<bool> {
    if entity_type != SESSIONS && entity_type != LINES {
        return Ok(false);
    }
    let Ok(received) = serde_json::from_str::<Map<String, Value>>(payload_json) else {
        return Ok(false);
    };
    let Some(local) = same_posting(conn, entity_type, entity_sync_id, &received).await? else {
        return Ok(false);
    };
    if entity_type == SESSIONS {
        write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, &received).await?;
    }
    close_outbox(
        conn,
        OutboxScope::Unsent(entity_type, entity_sync_id),
        "The same count was posted on another PC and reached the server first: its version is kept.".to_string(),
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
            "quantity": p.get("quantity"),
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
/// (who posted or reversed it, when, its reason), the version kept, and this PC.
async fn write_posted_twice_audit(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    local: &Map<String, Value>,
    kept: &Map<String, Value>,
) -> AppResult<()> {
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
                "summary": "The same stock count was posted on two PCs: the stock moved once; the version received first by the server is kept and this PC's posting is recorded here",
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
                "device_code": crate::sync::device_code::device_code_for(&replica_id),
                "replica_id": replica_id,
                "this_pc_posting": version_details(conn, entity_type, local).await?,
                "kept_posting": version_details(conn, entity_type, kept).await?,
            })),
        },
    )
    .await?;
    Ok(())
}

/// A received count movement or audit link that this PC already holds under the same seeded id,
/// for the same count, with other content (this PC posted the same count too and its push lost):
/// `true` tells the caller to write the received content over this PC's row (the version the server
/// received first is kept on every PC). This PC's own posting is audited ([`AUDIT_POSTED_TWICE`]).
/// The stock is not touched: an existing movement is never applied again
/// (`sync::apply_hooks::after_upsert` runs for inserts only).
pub(crate) async fn adopt_first_posted(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    inbound: &Map<String, Value>,
) -> AppResult<bool> {
    let Some(local) = same_seeded_count(conn, entity_type, entity_sync_id, inbound).await? else {
        return Ok(false);
    };
    if local == *inbound {
        return Ok(false);
    }
    write_posted_twice_audit(conn, entity_type, entity_sync_id, &local, inbound).await?;
    Ok(true)
}

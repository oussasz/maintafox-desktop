//! Hash chain of the audit trail (Phase 6.7).
//!
//! Every audit event written on this device takes the next position of the device's chain
//! (`chain_origin` = sync replica id, `chain_seq` = 1, 2, …) and stores
//! `chain_hash = SHA-256(chain_prev_hash ‖ canonical content)`, where `chain_prev_hash` is the
//! hash of the previous position. Changing an event, removing one or inserting one in the middle
//! breaks the chain from that position on.
//!
//! The canonical content is the row's sync payload (`sync::row_codec::load_row_payload`) without
//! `chain_hash` itself, the payload format marker and null fields: references are sync ids there,
//! so the same event hashes the same on every device that received it. Null fields are left out so
//! that a column added to the table later (null on older events) does not change older hashes.
//!
//! Events kept on their device (the `audit_events` row filter of `sync::row_spec`, or a target the
//! sync codec cannot encode) never travel: their content is hashed in its local form, the hash
//! prefixed [`LOCAL_FORM_PREFIX`] (`L2:`; `L1:` for events written before stubs existed). For each
//! of them the writer also records a hash-only stub (`audit_chain_stubs`, migration 250: origin,
//! position, previous hash, hash; no content), which travels. Another device verifies a chain it
//! received across those positions with the stubs:
//! - a stub must carry a local-form hash, point to the previous position's hash, and be what the
//!   next position points to. A travelling event removed and replaced by a forged stub is found:
//!   the next event points to a hash without the local prefix (and that pointer is hashed);
//! - removing a stub leaves a missing position, changing one breaks its links;
//! - positions missing without a stub are reported missing, except a run of positions kept on a
//!   device before stubs existed: when the chain of that device shows no stub and no `L2:` hash at
//!   all (that device has not upgraded, or its stubs have not arrived) and the position after the
//!   run points to an `L1:` hash, the run is counted as "kept on its device before stubs" and
//!   verification goes on after it. Only the last position of such a run is known to be local: a
//!   travelling event removed inside a longer run is not detected (KNOWN_LIMITS). Once that device
//!   upgrades, migration 250 sends stubs for those older positions and they are linked one by one.
//!
//! On the device that wrote the chain, stubs are not used: every event is there and verified
//! with its content.
//!
//! What the chain does not do: it has no secret key, so someone able to rewrite the whole
//! encrypted database could recompute a consistent chain; the copies already received by other
//! devices and the server still show the original chain.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::sync::row_codec::{load_row_payload, RowLocator};

/// Payload keys left out of the hashed content.
const UNHASHED_KEYS: &[&str] = &["chain_hash", crate::sync::row_layout::PAYLOAD_FORMAT_KEY];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("audit chain decode '{field}': {err}"))
}

/// `SHA-256(prev_hash ‖ canonical JSON)` in lowercase hex. `prev_hash` is empty for position 1.
pub fn chain_hash_of(prev_hash: Option<&str>, payload: &serde_json::Map<String, Value>) -> AppResult<String> {
    let canonical: BTreeMap<&str, &Value> = payload
        .iter()
        .filter(|(k, v)| !v.is_null() && !UNHASHED_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.unwrap_or("").as_bytes());
    hasher.update(b"\n");
    hasher.update(serde_json::to_string(&canonical)?.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

/// Canonical payload of one audit event (sync ids for references).
async fn event_payload(conn: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<serde_json::Map<String, Value>> {
    let layout = crate::sync::row_layout::layout(conn, "audit_events").await?;
    load_row_payload(conn, &layout, &RowLocator::Identity(entity_sync_id.to_string()))
        .await?
        .map(|p| p.payload)
        // Kept on this device by its row filter (`sync::row_spec`), or gone: hashed in its local
        // form (`event_hash`), which reads the row itself.
        .ok_or_else(|| AppError::SyncError(format!("audit event {entity_sync_id} has no sync payload")))
}

/// Marks a hash computed over the event's local form: events that never leave this device (kept
/// by the `audit_events` row filter, or a target type sync does not map) are hashed as stored
/// here, and get a hash-only stub that travels (`audit_chain_stubs`).
pub const LOCAL_FORM_PREFIX: &str = "L2:";

/// Local form of the events written before stubs existed (migration 250 gave them a stub).
pub const LOCAL_FORM_PREFIX_BEFORE_STUBS: &str = "L1:";

/// The local-form prefix a stored hash starts with, if any.
fn local_prefix(hash: &str) -> Option<&'static str> {
    [LOCAL_FORM_PREFIX, LOCAL_FORM_PREFIX_BEFORE_STUBS]
        .into_iter()
        .find(|p| hash.starts_with(p))
}

/// A stub's hash: a local-form prefix and a SHA-256 in lowercase hex.
fn is_local_form_hash(hash: &str) -> bool {
    local_prefix(hash).is_some_and(|p| {
        let digest = &hash[p.len()..];
        digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Columns of the local form (content and chain position; not `id`, not `chain_hash`).
const LOCAL_FORM_COLUMNS: &[&str] = &[
    "entity_sync_id",
    "action_code",
    "target_type",
    "target_id",
    "actor_id",
    "auth_context",
    "result",
    "before_hash",
    "after_hash",
    "happened_at",
    "retention_class",
    "details_json",
    "chain_origin",
    "chain_seq",
    "chain_prev_hash",
];

async fn local_form(conn: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<serde_json::Map<String, Value>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {} FROM audit_events WHERE entity_sync_id = ?",
                LOCAL_FORM_COLUMNS.join(", ")
            ),
            [entity_sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("audit event {entity_sync_id} missing")))?;
    let mut out = serde_json::Map::new();
    for col in LOCAL_FORM_COLUMNS {
        let v = if matches!(*col, "actor_id" | "chain_seq") {
            row.try_get::<Option<i64>>("", col)
                .map_err(|e| decode_err(col, e))?
                .map(Value::from)
        } else {
            row.try_get::<Option<String>>("", col)
                .map_err(|e| decode_err(col, e))?
                .map(Value::from)
        };
        out.insert((*col).to_string(), v.unwrap_or(Value::Null));
    }
    Ok(out)
}

/// Hash of an event at its chain position: over the sync payload when the codec can encode the
/// event, otherwise over the local form (prefixed with `local_prefix`).
async fn event_hash(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    prev_hash: Option<&str>,
    local_prefix: &str,
) -> AppResult<String> {
    match event_payload(conn, entity_sync_id).await {
        Ok(payload) => chain_hash_of(prev_hash, &payload),
        Err(AppError::SyncError(_)) => Ok(format!(
            "{local_prefix}{}",
            chain_hash_of(prev_hash, &local_form(conn, entity_sync_id).await?)?
        )),
        Err(other) => Err(other),
    }
}

/// Next position of this device's chain: `(origin, seq, prev_hash)`.
pub(crate) async fn next_position(conn: &impl ConnectionTrait) -> AppResult<(String, i64, Option<String>)> {
    let origin = crate::sync::queries::local_replica_id(conn).await?;
    let last = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT chain_seq, chain_hash FROM audit_events \
             WHERE chain_origin = ? AND chain_seq IS NOT NULL ORDER BY chain_seq DESC LIMIT 1",
            [origin.clone().into()],
        ))
        .await?;
    Ok(match last {
        None => (origin, 1, None),
        Some(r) => {
            let seq: i64 = r.try_get("", "chain_seq").map_err(|e| decode_err("chain_seq", e))?;
            let hash: Option<String> = r.try_get("", "chain_hash").map_err(|e| decode_err("chain_hash", e))?;
            let hash = hash.ok_or_else(|| {
                AppError::Internal(anyhow::anyhow!(
                    "audit chain position {seq} of this device has no hash; the chain cannot be extended"
                ))
            })?;
            (origin, seq + 1, Some(hash))
        }
    })
}

/// Whether `audit_chain_stubs` exists: not while the migrations before 250 run (migration 241
/// writes an audit event).
async fn stubs_table_exists(conn: &impl ConnectionTrait) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = 'audit_chain_stubs'".to_string(),
        ))
        .await?
        .is_some())
}

/// Seals an event inserted at its chain position (same transaction as the insert). An event kept
/// on this device (hashed in its local form) also gets its hash-only stub, which travels. An event
/// written by a migration that runs before the stubs table exists is sealed as an event from
/// before stubs (`L1:`): migration 250 gives it its stub.
pub(crate) async fn seal(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    origin: &str,
    seq: i64,
    prev_hash: Option<&str>,
) -> AppResult<()> {
    let with_stubs = stubs_table_exists(conn).await?;
    let prefix = if with_stubs {
        LOCAL_FORM_PREFIX
    } else {
        LOCAL_FORM_PREFIX_BEFORE_STUBS
    };
    let hash = event_hash(conn, entity_sync_id, prev_hash, prefix).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE audit_events SET chain_hash = ? WHERE entity_sync_id = ?",
        [hash.clone().into(), entity_sync_id.into()],
    ))
    .await?;
    if with_stubs && local_prefix(&hash).is_some() {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO audit_chain_stubs (entity_sync_id, chain_origin, chain_seq, chain_prev_hash, chain_hash) \
             VALUES (?, ?, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                origin.into(),
                seq.into(),
                prev_hash.map(str::to_string).into(),
                hash.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChainProblemKind {
    /// The stored hash does not match the event's content: the event was changed. For a position
    /// kept on its device (a stub): the stub's hash is not a local-form hash.
    HashMismatch,
    /// The event (or stub) does not point to the previous position's hash: a position was
    /// removed, inserted or replaced before it.
    PrevMismatch,
    /// Positions are missing before this one: removed here, or (for another device's chain)
    /// not received yet.
    MissingPositions,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainProblem {
    pub chain_seq: i64,
    /// The audit event at that position; `None` for a position kept on its device (a stub).
    pub audit_event_id: Option<i64>,
    pub kind: ChainProblemKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainOriginReport {
    /// Sync replica id of the device that wrote the chain.
    pub origin: String,
    /// Short device code (as in record numbers such as `OT-7KQ2-0001`).
    pub device_code: String,
    pub is_this_device: bool,
    pub events: i64,
    pub last_seq: i64,
    /// Positions kept on that device, linked through their stub (another device's chain only).
    pub kept_positions: i64,
    /// Positions kept on that device before stubs existed, with no stub here: accepted as a run
    /// before a position that points to a local-form hash, not linked one by one.
    pub kept_before_stubs: i64,
    pub intact: bool,
    pub first_problem: Option<ChainProblem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditChainReport {
    pub checked_at: String,
    pub chains: Vec<ChainOriginReport>,
    /// Events written before the chain existed (never chained, never rewritten).
    pub unchained_events: i64,
    pub intact: bool,
}

/// Verifies every device chain present in this database, from position 1 to its last position.
pub async fn verify_audit_chain(conn: &impl ConnectionTrait) -> AppResult<AuditChainReport> {
    let this_origin = crate::sync::queries::local_replica_id(conn).await?;
    let unchained = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM audit_events WHERE chain_seq IS NULL".to_string(),
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "n"))
        .transpose()
        .map_err(|e| decode_err("n", e))?
        .unwrap_or(0);
    // A device whose chain so far holds only positions kept on it is known here by its stubs.
    let origins = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT chain_origin FROM audit_events WHERE chain_seq IS NOT NULL \
             UNION SELECT chain_origin FROM audit_chain_stubs ORDER BY chain_origin"
                .to_string(),
        ))
        .await?;
    let mut chains = Vec::with_capacity(origins.len());
    for o in &origins {
        let origin: String = o
            .try_get("", "chain_origin")
            .map_err(|e| decode_err("chain_origin", e))?;
        let is_this_device = origin == this_origin;
        chains.push(verify_origin(conn, &origin, is_this_device).await?);
    }
    let intact = chains.iter().all(|c| c.intact);
    Ok(AuditChainReport {
        checked_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        chains,
        unchained_events: unchained,
        intact,
    })
}

/// One position of a chain as this database holds it.
enum Position {
    Event {
        id: i64,
        sync_id: String,
        prev: Option<String>,
        hash: Option<String>,
    },
    /// Kept on its device: only the stub is here.
    Stub { prev: Option<String>, hash: String },
}

impl Position {
    fn prev(&self) -> Option<&str> {
        match self {
            Position::Event { prev, .. } | Position::Stub { prev, .. } => prev.as_deref(),
        }
    }

    fn hash(&self) -> Option<&str> {
        match self {
            Position::Event { hash, .. } => hash.as_deref(),
            Position::Stub { hash, .. } => Some(hash),
        }
    }

    fn event_id(&self) -> Option<i64> {
        match self {
            Position::Event { id, .. } => Some(*id),
            Position::Stub { .. } => None,
        }
    }
}

async fn chain_positions(
    conn: &impl ConnectionTrait,
    origin: &str,
    with_stubs: bool,
) -> AppResult<(BTreeMap<i64, Position>, i64)> {
    let mut positions = BTreeMap::new();
    if with_stubs {
        let stubs = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT chain_seq, chain_prev_hash, chain_hash FROM audit_chain_stubs WHERE chain_origin = ?",
                [origin.into()],
            ))
            .await?;
        for r in &stubs {
            let seq: i64 = r.try_get("", "chain_seq").map_err(|e| decode_err("chain_seq", e))?;
            positions.insert(
                seq,
                Position::Stub {
                    prev: r
                        .try_get("", "chain_prev_hash")
                        .map_err(|e| decode_err("chain_prev_hash", e))?,
                    hash: r.try_get("", "chain_hash").map_err(|e| decode_err("chain_hash", e))?,
                },
            );
        }
    }
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, chain_seq, chain_prev_hash, chain_hash FROM audit_events \
             WHERE chain_origin = ? AND chain_seq IS NOT NULL",
            [origin.into()],
        ))
        .await?;
    // The event itself, when it is here, is verified with its content (it replaces its stub).
    for r in &rows {
        let seq: i64 = r.try_get("", "chain_seq").map_err(|e| decode_err("chain_seq", e))?;
        positions.insert(
            seq,
            Position::Event {
                id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                sync_id: r
                    .try_get("", "entity_sync_id")
                    .map_err(|e| decode_err("entity_sync_id", e))?,
                prev: r
                    .try_get("", "chain_prev_hash")
                    .map_err(|e| decode_err("chain_prev_hash", e))?,
                hash: r.try_get("", "chain_hash").map_err(|e| decode_err("chain_hash", e))?,
            },
        );
    }
    Ok((positions, i64::try_from(rows.len()).unwrap_or(i64::MAX)))
}

/// Whether an event's stored hash matches its content.
async fn event_hash_matches(
    conn: &impl ConnectionTrait,
    sync_id: &str,
    prev: Option<&str>,
    stored_hash: Option<&str>,
) -> AppResult<bool> {
    let actual = match stored_hash.and_then(local_prefix) {
        Some(prefix) => format!("{prefix}{}", chain_hash_of(prev, &local_form(conn, sync_id).await?)?),
        None => match event_payload(conn, sync_id).await {
            Ok(payload) => chain_hash_of(prev, &payload)?,
            // The payload form was used when sealed but can no longer be built: the event (or a
            // row it references) changed.
            Err(AppError::SyncError(_)) => return Ok(false),
            Err(other) => return Err(other),
        },
    };
    Ok(stored_hash == Some(actual.as_str()))
}

async fn verify_origin(
    conn: &impl ConnectionTrait,
    origin: &str,
    is_this_device: bool,
) -> AppResult<ChainOriginReport> {
    // This device holds every event of its own chain: its stubs are not needed to verify it.
    let (positions, events) = chain_positions(conn, origin, !is_this_device).await?;
    // Stubs exist for every position this device keeps from the version that writes them on
    // (migration 250 wrote them for older ones): once any is here, or any `L2:` hash, a position
    // missing without its stub is missing, never "kept before stubs".
    let has_stubs = positions.values().any(|p| matches!(p, Position::Stub { .. }))
        || positions.values().any(|p| {
            [p.prev(), p.hash()]
                .into_iter()
                .flatten()
                .any(|h| h.starts_with(LOCAL_FORM_PREFIX))
        });
    let mut expected_seq = 1_i64;
    let mut prev_hash: Option<String> = None;
    let mut first_problem = None;
    let mut last_seq = 0;
    let mut kept_positions = 0;
    let mut kept_before_stubs = 0;
    for (&seq, position) in &positions {
        last_seq = seq;
        if first_problem.is_some() {
            continue;
        }
        if seq != expected_seq
            && !is_this_device
            && !has_stubs
            && position
                .prev()
                .is_some_and(|p| p.starts_with(LOCAL_FORM_PREFIX_BEFORE_STUBS))
        {
            // A run kept on that device before stubs existed: the position after it points to a
            // local-form hash, which is all that can be known of the run.
            kept_before_stubs += seq - expected_seq;
            expected_seq = seq;
            prev_hash = position.prev().map(str::to_string);
        }
        let kind = if seq != expected_seq {
            Some(ChainProblemKind::MissingPositions)
        } else if position.prev() != prev_hash.as_deref() {
            Some(ChainProblemKind::PrevMismatch)
        } else {
            let matches = match position {
                Position::Event {
                    sync_id, prev, hash, ..
                } => event_hash_matches(conn, sync_id, prev.as_deref(), hash.as_deref()).await?,
                Position::Stub { hash, .. } => is_local_form_hash(hash),
            };
            (!matches).then_some(ChainProblemKind::HashMismatch)
        };
        match kind {
            Some(kind) => {
                first_problem = Some(ChainProblem {
                    chain_seq: seq,
                    audit_event_id: position.event_id(),
                    kind,
                });
            }
            None => {
                if matches!(position, Position::Stub { .. }) {
                    kept_positions += 1;
                }
                prev_hash = position.hash().map(str::to_string);
                expected_seq += 1;
            }
        }
    }
    Ok(ChainOriginReport {
        origin: origin.to_string(),
        device_code: crate::sync::device_code::device_code_for(origin),
        is_this_device,
        events,
        last_seq,
        kept_positions,
        kept_before_stubs,
        intact: first_problem.is_none(),
        first_problem,
    })
}

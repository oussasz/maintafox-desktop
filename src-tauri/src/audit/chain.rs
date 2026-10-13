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
//! so the same event hashes the same on every device that received it, and the chain of each
//! device can be verified anywhere. Null fields are left out so that a column added to the table
//! later (null on older events) does not change older hashes.
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("audit event {entity_sync_id} missing")))
}

/// Marks a hash computed over the event's local form: events whose target the sync codec cannot
/// encode (a target type sync does not map) never leave this device, so their content is hashed
/// as stored here. Verification uses the form the prefix names.
pub const LOCAL_FORM_PREFIX: &str = "L1:";

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
/// event, otherwise over the local form (prefixed with [`LOCAL_FORM_PREFIX`]).
async fn event_hash(conn: &impl ConnectionTrait, entity_sync_id: &str, prev_hash: Option<&str>) -> AppResult<String> {
    match event_payload(conn, entity_sync_id).await {
        Ok(payload) => chain_hash_of(prev_hash, &payload),
        Err(AppError::SyncError(_)) => Ok(format!(
            "{LOCAL_FORM_PREFIX}{}",
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

/// Seals an event inserted at its chain position (same transaction as the insert).
pub(crate) async fn seal(conn: &impl ConnectionTrait, entity_sync_id: &str, prev_hash: Option<&str>) -> AppResult<()> {
    let hash = event_hash(conn, entity_sync_id, prev_hash).await?;
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE audit_events SET chain_hash = ? WHERE entity_sync_id = ?",
        [hash.into(), entity_sync_id.into()],
    ))
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChainProblemKind {
    /// The stored hash does not match the event's content: the event was changed.
    HashMismatch,
    /// The event does not point to the previous event's hash: an event was removed, inserted
    /// or replaced before it.
    PrevMismatch,
    /// Positions are missing before this one: removed here, or (for another device's chain)
    /// not received yet.
    MissingPositions,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChainProblem {
    pub chain_seq: i64,
    pub audit_event_id: i64,
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
    let origins = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT chain_origin FROM audit_events WHERE chain_seq IS NOT NULL ORDER BY chain_origin"
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

async fn verify_origin(
    conn: &impl ConnectionTrait,
    origin: &str,
    is_this_device: bool,
) -> AppResult<ChainOriginReport> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, chain_seq, chain_prev_hash, chain_hash FROM audit_events \
             WHERE chain_origin = ? AND chain_seq IS NOT NULL ORDER BY chain_seq",
            [origin.into()],
        ))
        .await?;
    let mut expected_seq = 1_i64;
    let mut prev_hash: Option<String> = None;
    let mut first_problem = None;
    let mut last_seq = 0;
    for r in &rows {
        let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let sync_id: String = r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?;
        let seq: i64 = r.try_get("", "chain_seq").map_err(|e| decode_err("chain_seq", e))?;
        let stored_prev: Option<String> = r
            .try_get("", "chain_prev_hash")
            .map_err(|e| decode_err("chain_prev_hash", e))?;
        let stored_hash: Option<String> = r.try_get("", "chain_hash").map_err(|e| decode_err("chain_hash", e))?;
        last_seq = seq;
        if first_problem.is_some() {
            continue;
        }
        let kind = if seq != expected_seq {
            Some(ChainProblemKind::MissingPositions)
        } else if stored_prev != prev_hash {
            Some(ChainProblemKind::PrevMismatch)
        } else {
            let actual = if stored_hash.as_deref().is_some_and(|h| h.starts_with(LOCAL_FORM_PREFIX)) {
                format!(
                    "{LOCAL_FORM_PREFIX}{}",
                    chain_hash_of(stored_prev.as_deref(), &local_form(conn, &sync_id).await?)?
                )
            } else {
                match event_payload(conn, &sync_id).await {
                    Ok(payload) => chain_hash_of(stored_prev.as_deref(), &payload)?,
                    // The payload form was used when sealed but can no longer be built: the
                    // event (or a row it references) changed.
                    Err(AppError::SyncError(_)) => String::new(),
                    Err(other) => return Err(other),
                }
            };
            (stored_hash.as_deref() != Some(actual.as_str())).then_some(ChainProblemKind::HashMismatch)
        };
        match kind {
            Some(kind) => {
                first_problem = Some(ChainProblem {
                    chain_seq: seq,
                    audit_event_id: id,
                    kind,
                });
            }
            None => {
                prev_hash = stored_hash;
                expected_seq += 1;
            }
        }
    }
    Ok(ChainOriginReport {
        origin: origin.to_string(),
        device_code: crate::sync::device_code::device_code_for(origin),
        is_this_device,
        events: i64::try_from(rows.len()).unwrap_or(i64::MAX),
        last_seq,
        intact: first_problem.is_none(),
        first_problem,
    })
}

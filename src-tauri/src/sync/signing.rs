//! Per-user signing of sync changes, desktop side (sync-signing-design.md v9 §4, §7, §9).
//!
//! - **Enrolment** (§4): a user's key is enrolled with the sync server once per PC, with the
//!   password they just typed (sign-in, a second-signer password, or the sync centre's
//!   "Confirmez votre mot de passe pour envoyer vos modifications").
//! - **Signing** (§7, §9): before each push, every unsent item gets its maker's signature when the
//!   maker's enrolled key is held in this session (only the maker's own key: an item made in V's
//!   session waits for V), plus the signature of a second signer named in its author fields whose
//!   password was typed for it in this session.
//! - **Holding back**: an item with a maker that is not signed by them yet is not sent, nor are
//!   the later changes of the same record (they are built on it). It waits for the maker's next
//!   sign-in on this PC. Likewise an item that sets or changes an author field (or what that
//!   field vouches for) naming someone other than its maker is held until that person signed it
//!   (`sync_outbox.awaiting_signers_json`, design §9 "never sent"; the server's rule 4 would
//!   refuse it).

use base64::Engine as _;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;

use crate::auth::signing_keys::{self, ChangeSigningInput, UnlockedSigningKey};
use crate::errors::{AppError, AppResult};
use crate::sync::signing_rules::{author_payload_key, signing_rule};
use crate::sync::transport::wire::WireSignature;

/// At most this many signatures per item (server contract).
const MAX_SIGNATURES: usize = 8;

/// SQL over the outbox alias `o`: an earlier or this unsent change of the same record still waits
/// for its maker's signature, or for a second signer it names ([`sign_pending_items`] writes
/// `awaiting_signers_json`: `'[]'` when nobody else must sign, NULL while not judged yet, so an
/// item never judged is held, never sent).
pub(crate) const AWAITING_MAKER_SIGNATURE_SQL: &str = "EXISTS (
    SELECT 1 FROM sync_outbox w
    WHERE w.entity_type = o.entity_type AND w.entity_sync_id = o.entity_sync_id AND w.id <= o.id
      AND w.status IN ('pending', 'in_flight') AND w.maker_user_sync_id IS NOT NULL
      AND (COALESCE(w.awaiting_signers_json, 'unjudged') <> '[]'
           OR NOT EXISTS (
               SELECT 1 FROM json_each(COALESCE(w.signatures_json, '[]')) s
               WHERE json_extract(s.value, '$.user_sync_id') = w.maker_user_sync_id)))";

async fn key_is_enrolled(conn: &impl ConnectionTrait, key_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS ok FROM user_signing_keys_local
             WHERE key_id = ? AND enrolled_at IS NOT NULL AND retired_at IS NULL",
            [key_id.into()],
        ))
        .await?
        .is_some())
}

/// As the server's rule 4 (`same` of signing-rules.ts): missing and null are equal; an empty
/// string is not equal to null.
fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.unwrap_or(&Value::Null) == b.unwrap_or(&Value::Null)
}

/// The people other than `maker` whose signature the server's rule 4 requires on this payload:
/// each author field that is set or changed against `previous` (or whose guarded columns
/// changed) and names someone else. `previous = None`: the record is new to the server.
pub(crate) fn required_cosigners(
    entity_type: &str,
    payload_json: &str,
    previous_json: Option<&str>,
    maker: &str,
) -> Vec<String> {
    let mut required: Vec<String> = Vec::new();
    for (_, named) in required_cosigner_fields(entity_type, payload_json, previous_json, maker) {
        if !required.contains(&named) {
            required.push(named);
        }
    }
    required
}

/// [`required_cosigners`] with the payload key of each author field: `(field, named person)`.
pub(crate) fn required_cosigner_fields(
    entity_type: &str,
    payload_json: &str,
    previous_json: Option<&str>,
    maker: &str,
) -> Vec<(String, String)> {
    let Some(rule) = signing_rule(entity_type) else {
        return Vec::new();
    };
    let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(payload_json) else {
        return Vec::new();
    };
    let previous = previous_json.and_then(|p| match serde_json::from_str::<Value>(p) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    });
    let before = |key: &str| previous.as_ref().and_then(|p| p.get(key));
    let mut required: Vec<(String, String)> = Vec::new();
    for field in rule.author_fields {
        let key = author_payload_key(field);
        let value = payload.get(&key);
        let changed = !same(value, before(&key))
            || field
                .guards
                .iter()
                .chain(field.vouches)
                .any(|g| !same(payload.get(*g), before(g)));
        if !changed {
            continue;
        }
        if let Some(named) = value.and_then(Value::as_str).filter(|v| !v.is_empty()) {
            if named != maker {
                required.push((key, named.to_string()));
            }
        }
    }
    required
}

/// The payload of the version this item was built on, as the server last knew it from this PC:
/// the latest earlier outbox item of the record, or the latest version received from the server,
/// whichever is newer. `None` when the record is new to the server (or its last version deletes it).
async fn previous_payload(conn: &impl ConnectionTrait, item: &PendingItem) -> AppResult<Option<String>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT payload_json, operation FROM (
                 SELECT payload_json, operation, row_version, 0 AS source FROM sync_outbox
                 WHERE entity_type = ? AND entity_sync_id = ? AND id < ?
                   AND status IN ('pending', 'in_flight', 'acked')
                 UNION ALL
                 SELECT payload_json, operation, row_version, 1 AS source FROM sync_inbox
                 WHERE entity_type = ? AND entity_sync_id = ? AND apply_status = 'applied'
             )
             ORDER BY row_version DESC, source ASC LIMIT 1",
            [
                item.entity_type.clone().into(),
                item.entity_sync_id.clone().into(),
                item.id.into(),
                item.entity_type.clone().into(),
                item.entity_sync_id.clone().into(),
            ],
        ))
        .await?;
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("sync signing previous version: {e}"));
    let Some(row) = row else {
        return Ok(None);
    };
    let operation: String = row.try_get("", "operation").map_err(read)?;
    if operation == "delete" {
        return Ok(None);
    }
    Ok(Some(row.try_get("", "payload_json").map_err(read)?))
}

struct PendingItem {
    id: i64,
    entity_type: String,
    entity_sync_id: String,
    operation: String,
    row_version: i64,
    base_row_version: Option<i64>,
    payload_json: String,
    maker: String,
    signatures: Vec<WireSignature>,
}

fn sign_item(item: &PendingItem, tenant_id: &str, key: &UnlockedSigningKey) -> AppResult<WireSignature> {
    let signature = key.sign_change(&ChangeSigningInput {
        tenant_id,
        entity_type: &item.entity_type,
        entity_sync_id: &item.entity_sync_id,
        operation: &item.operation,
        row_version: item.row_version,
        base_row_version: item.base_row_version,
        payload_json: &item.payload_json,
        signer_user_sync_id: key.user_sync_id(),
        key_id: key.key_id(),
    })?;
    Ok(WireSignature {
        user_sync_id: key.user_sync_id().to_string(),
        key_id: key.key_id().to_string(),
        signature,
    })
}

/// Signs the unsent items this session can sign (maker first, then named second signers).
/// Returns how many items gained a signature.
pub async fn sign_pending_items(conn: &impl ConnectionTrait, tenant_id: &str) -> AppResult<i64> {
    let rows = conn
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type, entity_sync_id, operation, row_version, base_row_version, payload_json,
                    maker_user_sync_id, signatures_json
                    , awaiting_signers_json
             FROM sync_outbox
             WHERE status = 'pending' AND maker_user_sync_id IS NOT NULL
             ORDER BY id"
                .to_string(),
        ))
        .await?;
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("sync_outbox signing: {e}"));
    let mut signed_items = 0_i64;
    for row in &rows {
        let signatures_json: Option<String> = row.try_get("", "signatures_json").map_err(read)?;
        let item = PendingItem {
            id: row.try_get("", "id").map_err(read)?,
            entity_type: row.try_get("", "entity_type").map_err(read)?,
            entity_sync_id: row.try_get("", "entity_sync_id").map_err(read)?,
            operation: row.try_get("", "operation").map_err(read)?,
            row_version: row.try_get("", "row_version").map_err(read)?,
            base_row_version: row.try_get("", "base_row_version").map_err(read)?,
            payload_json: row.try_get("", "payload_json").map_err(read)?,
            maker: row.try_get("", "maker_user_sync_id").map_err(read)?,
            signatures: signatures_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        };
        let mut signatures = item.signatures.clone();
        let mut changed = false;
        let has = |sigs: &[WireSignature], user: &str| sigs.iter().any(|s| s.user_sync_id == user);

        if !has(&signatures, &item.maker) {
            if let Some(key) = signing_keys::held_key(&item.maker) {
                if key_is_enrolled(conn, key.key_id()).await? {
                    signatures.insert(0, sign_item(&item, tenant_id, &key)?);
                    changed = true;
                }
            }
        }
        // Who else must still sign before the item may be sent (server rule 4, review F3): a
        // second person named by a set or changed author field, until their signature is there
        // and their key is enrolled with the server (a co-signature is stored when the action
        // commits, `cosign_committed`, possibly before the enrolment). A delete names nobody.
        let mut awaiting: Vec<String> = Vec::new();
        if item.operation != "delete" {
            let previous = previous_payload(conn, &item).await?;
            for named in required_cosigners(&item.entity_type, &item.payload_json, previous.as_deref(), &item.maker) {
                let ready = match signatures.iter().find(|s| s.user_sync_id == named) {
                    Some(sig) => key_is_enrolled(conn, &sig.key_id).await?,
                    None => false,
                };
                if !ready {
                    awaiting.push(named);
                }
            }
        }
        let awaiting_json = serde_json::to_string(&awaiting)?;
        let stored_awaiting: Option<String> = row.try_get("", "awaiting_signers_json").map_err(read)?;
        if changed || stored_awaiting.as_deref() != Some(awaiting_json.as_str()) {
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox SET signatures_json = ?, awaiting_signers_json = ?,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ? AND status = 'pending'",
                [
                    if signatures.is_empty() {
                        None::<String>.into()
                    } else {
                        Some(serde_json::to_string(&signatures)?).into()
                    },
                    awaiting_json.into(),
                    item.id.into(),
                ],
            ))
            .await?;
        }
        if changed {
            signed_items += 1;
        }
    }
    Ok(signed_items)
}

// ── Co-signature of a committed action (review F2, review of 5feb40ade) ─────────────────────

/// The record a second signer's password was typed for, who made the change, and the author
/// field (payload key) the action names them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CosignGrant {
    pub entity_type: String,
    pub entity_sync_id: String,
    pub maker_user_sync_id: String,
    pub author_field: String,
}

/// Right after the action committed: stages its captured changes and signs, with the second
/// signer's key, exactly the items of that record by that maker staged after the password check
/// (`after_outbox_id`) whose set or changed author fields name the second signer. The signature is
/// stored at once (the key is then dropped), so the item no longer depends on the second signer
/// staying here; it is sent once their key is enrolled. Returns how many items were co-signed.
pub async fn cosign_committed(
    db: &DatabaseConnection,
    key: &UnlockedSigningKey,
    grant: &CosignGrant,
    after_outbox_id: i64,
    tenant_id: &str,
) -> AppResult<i64> {
    use sea_orm::TransactionTrait;
    let txn = db.begin().await?;
    crate::sync::capture::drain_capture_journal(&txn).await?;
    let rows = txn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, entity_type, entity_sync_id, operation, row_version, base_row_version, payload_json,
                    maker_user_sync_id, signatures_json
             FROM sync_outbox
             WHERE status = 'pending' AND entity_type = ? AND entity_sync_id = ?
               AND maker_user_sync_id = ? AND id > ?
             ORDER BY id",
            [
                grant.entity_type.clone().into(),
                grant.entity_sync_id.clone().into(),
                grant.maker_user_sync_id.clone().into(),
                after_outbox_id.into(),
            ],
        ))
        .await?;
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("sync_outbox co-signing: {e}"));
    let mut cosigned = 0_i64;
    for row in &rows {
        let signatures_json: Option<String> = row.try_get("", "signatures_json").map_err(read)?;
        let item = PendingItem {
            id: row.try_get("", "id").map_err(read)?,
            entity_type: row.try_get("", "entity_type").map_err(read)?,
            entity_sync_id: row.try_get("", "entity_sync_id").map_err(read)?,
            operation: row.try_get("", "operation").map_err(read)?,
            row_version: row.try_get("", "row_version").map_err(read)?,
            base_row_version: row.try_get("", "base_row_version").map_err(read)?,
            payload_json: row.try_get("", "payload_json").map_err(read)?,
            maker: row.try_get("", "maker_user_sync_id").map_err(read)?,
            signatures: signatures_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        };
        if item.operation == "delete"
            || item.signatures.len() >= MAX_SIGNATURES
            || item.signatures.iter().any(|s| s.user_sync_id == key.user_sync_id())
        {
            continue;
        }
        let previous = previous_payload(&txn, &item).await?;
        // Only an item where the action's author field names them, and no other changed author
        // field does: an earlier change of the same record (still in the capture journal when
        // the password was typed, so staged with this one) naming them elsewhere is not theirs
        // to vouch for (review of 31c3820f8, LOW 3).
        let theirs: Vec<String> =
            required_cosigner_fields(&item.entity_type, &item.payload_json, previous.as_deref(), &item.maker)
                .into_iter()
                .filter(|(_, named)| named == key.user_sync_id())
                .map(|(field, _)| field)
                .collect();
        if theirs.is_empty() || theirs.iter().any(|field| *field != grant.author_field) {
            continue;
        }
        let mut signatures = item.signatures.clone();
        signatures.push(sign_item(&item, tenant_id, key)?);
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_outbox SET signatures_json = ?, awaiting_signers_json = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND status = 'pending'",
            [serde_json::to_string(&signatures)?.into(), item.id.into()],
        ))
        .await?;
        cosigned += 1;
    }
    txn.commit().await?;
    Ok(cosigned)
}

// ── Session end (review F1) ─────────────────────────────────────────────────────────────────

/// Ends per-user signing for a session that ended, however it ended (sign-out, stored-state check,
/// lockout, stale tenant claim, licence reset, failed sign-in): every held key and typed co-signer
/// password is released, and the capture actor is cleared so nobody is the maker of later changes.
/// Even if clearing the actor fails, it is inert once the session row is closed or past its expiry
/// (`sync::capture::LIVE_CAPTURE_ACTOR_SQL`).
pub async fn end_signing_session(db: &DatabaseConnection) -> AppResult<()> {
    signing_keys::release_session_keys();
    crate::auth::second_signer::release_pending_enrolments();
    crate::sync::capture::set_capture_actor(db, None).await
}

/// [`end_signing_session`] for paths that must not fail on it (the session is already ended).
pub async fn end_signing_session_logged(db: &DatabaseConnection, context: &'static str) {
    if let Err(e) = end_signing_session(db).await {
        tracing::error!(error = %e, context, "sync signing: capture actor not cleared at session end");
    }
}

// ── Enrolment (design §4) ────────────────────────────────────────────────────────────────

/// What the server answered to an enrolment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrolOutcome {
    Enrolled,
    /// Wrong password, inactive account, seeded admin with its default password, or bad proof.
    Refused,
    /// Too many failed attempts on this PC: try again later.
    Locked,
    /// The PC is not activated or the server cannot be reached: try again later.
    Unavailable,
}

const ENROL_PATH: &str = "/api/v1/sync/user-keys/enrol";

/// Enrols `key` with the sync server, proving the password of its user. The password is sent over
/// TLS only for this check (the server never stores it, design §4 / KNOWN_LIMITS).
pub async fn enrol_key(db: &DatabaseConnection, key: &UnlockedSigningKey, password: &str) -> AppResult<EnrolOutcome> {
    let Some(claim) = crate::commands::product_license::get_activation_claim_record(db).await? else {
        return Ok(EnrolOutcome::Unavailable);
    };
    let Some(token) = crate::commands::product_license::sync_activation_bearer_token(db).await? else {
        return Ok(EnrolOutcome::Unavailable);
    };
    let proof = key.sign(&signing_keys::enrol_proof_message(
        &claim.tenant_id,
        key.user_sync_id(),
        key.key_id(),
        &claim.machine_fingerprint,
    ))?;
    // Serialised straight from borrowed fields: the request body is the only copy of the password
    // made here (no intermediate JSON value). That body is handed to the HTTP client, which owns
    // and frees it (review F8; it cannot be wiped from here).
    #[derive(serde::Serialize)]
    struct EnrolBody<'a> {
        user_sync_id: &'a str,
        public_key: String,
        password: &'a str,
        proof_signature: String,
    }
    let body = serde_json::to_string(&EnrolBody {
        user_sync_id: key.user_sync_id(),
        public_key: key.public_key_b64(),
        password,
        proof_signature: base64::engine::general_purpose::STANDARD.encode(proof),
    })?;
    let client = crate::sync::transport::client::ControlPlaneClient::new(
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        crate::sync::transport::SyncTiming::production().timeouts,
    )
    .map_err(|e| AppError::SyncError(format!("sync client: {e}")))?;
    let outcome = match client.post_json_raw(ENROL_PATH, Some(&token), body).await {
        Ok((status, _)) if (200..300).contains(&status) => EnrolOutcome::Enrolled,
        // The same key enrolled by an earlier attempt whose answer was lost.
        Ok((409, text)) if text.contains("key_already_enrolled") => EnrolOutcome::Enrolled,
        Ok((403, _)) => EnrolOutcome::Refused,
        Ok((429, _)) => EnrolOutcome::Locked,
        Ok((status, _)) => {
            tracing::warn!(status, "sync signing: enrolment answered unexpectedly");
            EnrolOutcome::Unavailable
        }
        Err(e) => {
            tracing::warn!(error = %e.message(), "sync signing: enrolment not reached");
            EnrolOutcome::Unavailable
        }
    };
    if outcome == EnrolOutcome::Enrolled {
        signing_keys::mark_enrolled(db, key.key_id()).await?;
    }
    Ok(outcome)
}

/// Enrols the key in the background when it is not enrolled yet (never delays the action that
/// unlocked it). The password copy is wiped when done.
pub fn spawn_enrolment_if_needed(db: DatabaseConnection, key: std::sync::Arc<UnlockedSigningKey>, password: String) {
    tauri::async_runtime::spawn(async move {
        let mut password = password;
        match key_is_enrolled(&db, key.key_id()).await {
            Ok(true) => {}
            Ok(false) => match enrol_key(&db, &key, &password).await {
                Ok(outcome) => tracing::info!(?outcome, key_id = %key.key_id(), "sync signing: enrolment"),
                Err(e) => tracing::warn!(error = %e, "sync signing: enrolment failed"),
            },
            Err(e) => tracing::warn!(error = %e, "sync signing: enrolment state unreadable"),
        }
        wipe_string(&mut password);
    });
}

pub(crate) fn wipe_string(s: &mut String) {
    // SAFETY: zero bytes are valid UTF-8, so the string stays valid while it is overwritten.
    for b in unsafe { s.as_bytes_mut() } {
        // SAFETY: `b` is a valid, exclusive reference into the string's buffer.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    s.clear();
}

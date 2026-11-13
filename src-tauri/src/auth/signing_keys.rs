//! Per-user sync signing keys (sync-signing-design.md v9 §3, §4, §7).
//!
//! One Ed25519 key per (user, PC). The private key (PKCS#8) is wrapped with AES-256-GCM under a key
//! derived with Argon2id from the user's password and its own salt, and stored in the device-local
//! table `user_signing_keys_local` (never synced, not in the Windows keyring: plant PCs often
//! share one Windows account). It is unwrapped when the user signs in with their password, held in
//! memory for the session and wiped at sign-out.
//!
//! A key is bound to the account's password hash at creation (`password_hash_digest`): any
//! password change, reset or reactivation gives a new key at the next sign-in, and the server
//! revokes the old one when it judges that change (§6). An old key is never re-attested.
//!
//! Signed messages (§7): an ASCII tag, then each field as `<u32 big-endian length><UTF-8 bytes>`.
//! The payload digest is always recomputed from `payload_json`. The cross-language vector
//! `auth/testdata/sync-signing-vector.json` is shared with the sync server.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{self, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};

pub const CHANGE_SIGNATURE_TAG: &str = "mfx-sync-sig-v1";
pub const ENROL_PROOF_TAG: &str = "mfx-user-key-enrol-v1";
const WRAP_AAD_TAG: &str = "mfx-user-key-wrap-v1";

/// Argon2id cost of the wrapping key (below the login hash cost: the password is already checked
/// against the account hash before a key is unwrapped).
const WRAP_MEMORY_KIB: u32 = 19 * 1024;
const WRAP_TIME_COST: u32 = 2;
const WRAP_PARALLELISM: u32 = 1;
const WRAP_SALT_BYTES: usize = 16;

/// `<tag><u32 BE len><utf8>...` (design §7).
pub fn encode_signed(tag: &str, fields: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in std::iter::once(tag).chain(fields.iter().copied()) {
        let bytes = value.as_bytes();
        let len = u32::try_from(bytes.len()).expect("signed field longer than 4 GiB");
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(bytes);
    }
    out
}

/// What a user signs for one change.
#[derive(Debug, Clone, Copy)]
pub struct ChangeSigningInput<'a> {
    pub tenant_id: &'a str,
    pub entity_type: &'a str,
    pub entity_sync_id: &'a str,
    pub operation: &'a str,
    pub row_version: i64,
    pub base_row_version: Option<i64>,
    pub payload_json: &'a str,
    pub signer_user_sync_id: &'a str,
    pub key_id: &'a str,
}

/// The bytes a user signs for one change (design §7).
pub fn change_signing_message(input: &ChangeSigningInput<'_>) -> Vec<u8> {
    let row_version = input.row_version.to_string();
    let base = input.base_row_version.map(|v| v.to_string()).unwrap_or_default();
    let digest = hex::encode(Sha256::digest(input.payload_json.as_bytes()));
    encode_signed(
        CHANGE_SIGNATURE_TAG,
        &[
            input.tenant_id,
            input.entity_type,
            input.entity_sync_id,
            input.operation,
            &row_version,
            &base,
            &digest,
            input.signer_user_sync_id,
            input.key_id,
        ],
    )
}

/// The bytes signed at enrolment to prove possession of the private key (design §4).
pub fn enrol_proof_message(tenant_id: &str, user_sync_id: &str, key_id: &str, machine_fingerprint: &str) -> Vec<u8> {
    encode_signed(ENROL_PROOF_TAG, &[tenant_id, user_sync_id, key_id, machine_fingerprint])
}

/// `key_id` = hex of the first 16 bytes of SHA-256(raw public key).
pub fn key_id_of(raw_public_key: &[u8]) -> String {
    hex::encode(&Sha256::digest(raw_public_key)[..16])
}

/// SHA-256 hex of the account's stored password hash (PHC string): binds a key to one password.
pub fn password_hash_digest(password_hash: &str) -> String {
    hex::encode(Sha256::digest(password_hash.as_bytes()))
}

/// True when `signature` is a valid Ed25519 signature of `message` by `raw_public_key`.
pub fn verify_signature(raw_public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    UnparsedPublicKey::new(&signature::ED25519, raw_public_key)
        .verify(message, signature)
        .is_ok()
}

fn crypto_err(what: &str) -> AppError {
    AppError::Auth(format!("signing key: {what}"))
}

/// Overwrites a buffer before it is freed (the private key must not linger in freed memory).
fn wipe(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` is a valid, exclusive reference into `buf`.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// An unwrapped private key, held for the session of its user. Wiped on drop; never printed.
pub struct UnlockedSigningKey {
    user_sync_id: String,
    key_id: String,
    public_key: Vec<u8>,
    pkcs8: Vec<u8>,
}

impl fmt::Debug for UnlockedSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnlockedSigningKey")
            .field("user_sync_id", &self.user_sync_id)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl Drop for UnlockedSigningKey {
    fn drop(&mut self) {
        wipe(&mut self.pkcs8);
    }
}

impl UnlockedSigningKey {
    fn from_pkcs8(user_sync_id: &str, pkcs8: Vec<u8>) -> AppResult<Self> {
        let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| crypto_err("invalid private key"))?;
        let public_key = pair.public_key().as_ref().to_vec();
        Ok(Self {
            user_sync_id: user_sync_id.to_string(),
            key_id: key_id_of(&public_key),
            public_key,
            pkcs8,
        })
    }

    pub fn user_sync_id(&self) -> &str {
        &self.user_sync_id
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn public_key_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.public_key)
    }

    /// Ed25519 signature of `message` (64 bytes).
    pub fn sign(&self, message: &[u8]) -> AppResult<Vec<u8>> {
        let pair = Ed25519KeyPair::from_pkcs8(&self.pkcs8).map_err(|_| crypto_err("invalid private key"))?;
        Ok(pair.sign(message).as_ref().to_vec())
    }

    /// Signature of one change by this key's user, base64 (design §7).
    pub fn sign_change(&self, change: &ChangeSigningInput<'_>) -> AppResult<String> {
        if change.signer_user_sync_id != self.user_sync_id || change.key_id != self.key_id {
            return Err(crypto_err("change names another signer"));
        }
        let sig = self.sign(&change_signing_message(change))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(sig))
    }
}

/// A private key wrapped for storage.
#[derive(Debug, Clone)]
pub struct WrappedKey {
    pub kdf_salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub kdf_params: String,
}

fn wrap_kdf_params() -> String {
    format!("argon2id;m={WRAP_MEMORY_KIB};t={WRAP_TIME_COST};p={WRAP_PARALLELISM}")
}

fn derive_wrapping_key(password: &str, salt: &[u8], kdf_params: &str) -> AppResult<LessSafeKey> {
    if kdf_params != wrap_kdf_params() {
        return Err(crypto_err("unknown key derivation parameters"));
    }
    let params = Params::new(WRAP_MEMORY_KIB, WRAP_TIME_COST, WRAP_PARALLELISM, Some(32))
        .map_err(|_| crypto_err("key derivation parameters"))?;
    let mut key = [0u8; 32];
    let derived = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| crypto_err("key derivation"));
    let unbound =
        derived.and_then(|()| UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| crypto_err("wrapping key")));
    wipe(&mut key);
    Ok(LessSafeKey::new(unbound?))
}

fn wrap_aad(user_sync_id: &str, key_id: &str) -> Vec<u8> {
    encode_signed(WRAP_AAD_TAG, &[user_sync_id, key_id])
}

/// Creates a new key for `user_sync_id` and wraps it under `password`.
pub fn generate_wrapped(password: &str, user_sync_id: &str) -> AppResult<(UnlockedSigningKey, WrappedKey)> {
    let rng = SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| crypto_err("key generation"))?;
    let key = UnlockedSigningKey::from_pkcs8(user_sync_id, doc.as_ref().to_vec())?;
    let wrapped = wrap(password, &key, &rng)?;
    Ok((key, wrapped))
}

fn wrap(password: &str, key: &UnlockedSigningKey, rng: &SystemRandom) -> AppResult<WrappedKey> {
    let mut salt = vec![0u8; WRAP_SALT_BYTES];
    rng.fill(&mut salt).map_err(|_| crypto_err("random salt"))?;
    let mut nonce = [0u8; aead::NONCE_LEN];
    rng.fill(&mut nonce).map_err(|_| crypto_err("random nonce"))?;
    let kdf_params = wrap_kdf_params();
    let sealing = derive_wrapping_key(password, &salt, &kdf_params)?;
    let mut buf = key.pkcs8.clone();
    let sealed = sealing.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(wrap_aad(&key.user_sync_id, &key.key_id)),
        &mut buf,
    );
    if sealed.is_err() {
        wipe(&mut buf);
        return Err(crypto_err("wrap"));
    }
    Ok(WrappedKey {
        kdf_salt: salt,
        nonce: nonce.to_vec(),
        ciphertext: buf,
        kdf_params,
    })
}

/// Unwraps a stored key with `password`; fails when the password is wrong or the stored row was
/// altered (the AEAD binds the user and the key id; the public key is re-derived and compared).
pub fn unwrap_key(
    password: &str,
    user_sync_id: &str,
    key_id: &str,
    public_key: &[u8],
    wrapped: &WrappedKey,
) -> AppResult<UnlockedSigningKey> {
    let opening = derive_wrapping_key(password, &wrapped.kdf_salt, &wrapped.kdf_params)?;
    let nonce: [u8; aead::NONCE_LEN] = wrapped
        .nonce
        .as_slice()
        .try_into()
        .map_err(|_| crypto_err("stored nonce"))?;
    let mut buf = wrapped.ciphertext.clone();
    let opened = opening
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(wrap_aad(user_sync_id, key_id)),
            &mut buf,
        )
        .map(|plain| plain.to_vec());
    wipe(&mut buf);
    let pkcs8 = opened.map_err(|_| crypto_err("cannot unwrap (wrong password or altered key)"))?;
    let key = UnlockedSigningKey::from_pkcs8(user_sync_id, pkcs8)?;
    if key.key_id != key_id || key.public_key != public_key {
        return Err(crypto_err("stored key does not match its public key"));
    }
    Ok(key)
}

// ── Keys held in memory ────────────────────────────────────────────────────────────────────
//
// The signed-in user's key (`Store::Session`), and a second signer's key for the four-eyes step
// that needed it (`Store::Cosigner`), are held here by user sync id so the sync push can sign the
// session's own changes. Every key is released when the session is created or cleared
// (`SessionManager`); dropping the last reference wipes it. Tests keep one store per thread
// (each `#[tokio::test]` runs on its own thread), so parallel tests never share keys.

type KeyMap = std::collections::HashMap<String, std::sync::Arc<UnlockedSigningKey>>;

#[derive(Clone, Copy)]
enum Store {
    Session,
    Cosigner,
}

#[cfg(not(test))]
fn with_store<R>(store: Store, f: impl FnOnce(&mut KeyMap) -> R) -> Option<R> {
    static SESSION: std::sync::OnceLock<std::sync::Mutex<KeyMap>> = std::sync::OnceLock::new();
    static COSIGNER: std::sync::OnceLock<std::sync::Mutex<KeyMap>> = std::sync::OnceLock::new();
    let cell = match store {
        Store::Session => &SESSION,
        Store::Cosigner => &COSIGNER,
    };
    cell.get_or_init(Default::default)
        .lock()
        .ok()
        .map(|mut keys| f(&mut keys))
}

#[cfg(test)]
fn with_store<R>(store: Store, f: impl FnOnce(&mut KeyMap) -> R) -> Option<R> {
    thread_local! {
        static SESSION: std::cell::RefCell<KeyMap> = std::cell::RefCell::new(KeyMap::new());
        static COSIGNER: std::cell::RefCell<KeyMap> = std::cell::RefCell::new(KeyMap::new());
    }
    let cell = match store {
        Store::Session => &SESSION,
        Store::Cosigner => &COSIGNER,
    };
    Some(cell.with(|keys| f(&mut keys.borrow_mut())))
}

/// Holds `key` for its user until the session ends.
pub fn hold_session_key(key: UnlockedSigningKey) -> std::sync::Arc<UnlockedSigningKey> {
    let key = std::sync::Arc::new(key);
    with_store(Store::Session, |keys| {
        keys.insert(key.user_sync_id.clone(), key.clone())
    });
    key
}

/// The held key of `user_sync_id`, if that user signed in in this session.
pub fn held_key(user_sync_id: &str) -> Option<std::sync::Arc<UnlockedSigningKey>> {
    with_store(Store::Session, |keys| keys.get(user_sync_id).cloned()).flatten()
}

/// Holds a second signer's key after they typed their password for an action of this session
/// (`auth::second_signer`): it co-signs the session's items that name them in an author field,
/// and is released once it signed (or with the session).
pub fn hold_cosigner_key(key: UnlockedSigningKey) {
    let key = std::sync::Arc::new(key);
    with_store(Store::Cosigner, |keys| {
        keys.insert(key.user_sync_id.clone(), key.clone())
    });
}

/// The held co-signer key of `user_sync_id`.
pub fn cosigner_key(user_sync_id: &str) -> Option<std::sync::Arc<UnlockedSigningKey>> {
    with_store(Store::Cosigner, |keys| keys.get(user_sync_id).cloned()).flatten()
}

/// Releases one co-signer key (it signed what it was typed for).
pub fn release_cosigner_key(user_sync_id: &str) {
    with_store(Store::Cosigner, |keys| keys.remove(user_sync_id));
}

/// Releases every held key (sign-out, session expiry, tenant change, new session).
pub fn release_session_keys() {
    with_store(Store::Session, KeyMap::clear);
    with_store(Store::Cosigner, KeyMap::clear);
}

// ── Device-local storage (`user_signing_keys_local`, migration 297) ──────────────────────────

/// One stored key of this PC.
#[derive(Debug, Clone)]
pub struct LocalKeyRow {
    pub id: i64,
    pub user_id: i64,
    pub user_sync_id: String,
    pub key_id: String,
    pub public_key: Vec<u8>,
    pub wrapped: WrappedKey,
    pub password_hash_digest: String,
    pub enrolled_at: Option<String>,
}

fn stmt(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn read_err(e: sea_orm::DbErr) -> AppError {
    AppError::SyncError(format!("user_signing_keys_local: {e}"))
}

fn row_to_key(row: &sea_orm::QueryResult) -> AppResult<LocalKeyRow> {
    Ok(LocalKeyRow {
        id: row.try_get("", "id").map_err(read_err)?,
        user_id: row.try_get("", "user_id").map_err(read_err)?,
        user_sync_id: row.try_get("", "user_sync_id").map_err(read_err)?,
        key_id: row.try_get("", "key_id").map_err(read_err)?,
        public_key: row.try_get("", "public_key").map_err(read_err)?,
        wrapped: WrappedKey {
            kdf_salt: row.try_get("", "kdf_salt").map_err(read_err)?,
            nonce: row.try_get("", "wrap_nonce").map_err(read_err)?,
            ciphertext: row.try_get("", "wrapped_private_key").map_err(read_err)?,
            kdf_params: row.try_get("", "kdf_params").map_err(read_err)?,
        },
        password_hash_digest: row.try_get("", "password_hash_digest").map_err(read_err)?,
        enrolled_at: row.try_get("", "enrolled_at").map_err(read_err)?,
    })
}

const KEY_COLUMNS: &str = "id, user_id, user_sync_id, key_id, public_key, kdf_salt, wrap_nonce, \
    wrapped_private_key, kdf_params, password_hash_digest, enrolled_at";

/// The current (not retired) key of a user on this PC.
pub async fn current_local_key(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<Option<LocalKeyRow>> {
    let row = conn
        .query_one(stmt(
            &format!(
                "SELECT {KEY_COLUMNS} FROM user_signing_keys_local
                 WHERE user_id = ? AND retired_at IS NULL ORDER BY id DESC LIMIT 1"
            ),
            vec![user_id.into()],
        ))
        .await?;
    row.as_ref().map(row_to_key).transpose()
}

/// Marks the user's keys retired (a new one is created at the next unlock).
pub async fn retire_local_keys(conn: &impl ConnectionTrait, user_id: i64, reason: &str) -> AppResult<()> {
    conn.execute(stmt(
        "UPDATE user_signing_keys_local
         SET retired_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), retired_reason = ?
         WHERE user_id = ? AND retired_at IS NULL",
        vec![reason.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}

/// Records that the server accepted this key's enrolment.
pub async fn mark_enrolled(conn: &impl ConnectionTrait, key_id: &str) -> AppResult<()> {
    conn.execute(stmt(
        "UPDATE user_signing_keys_local
         SET enrolled_at = COALESCE(enrolled_at, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         WHERE key_id = ?",
        vec![key_id.into()],
    ))
    .await?;
    Ok(())
}

async fn insert_local_key(
    conn: &impl ConnectionTrait,
    user_id: i64,
    key: &UnlockedSigningKey,
    wrapped: &WrappedKey,
    password_hash_digest: &str,
) -> AppResult<()> {
    conn.execute(stmt(
        "INSERT INTO user_signing_keys_local
            (user_id, user_sync_id, key_id, public_key, kdf_salt, wrap_nonce, wrapped_private_key,
             kdf_params, password_hash_digest, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        vec![
            user_id.into(),
            key.user_sync_id.clone().into(),
            key.key_id.clone().into(),
            key.public_key.clone().into(),
            wrapped.kdf_salt.clone().into(),
            wrapped.nonce.clone().into(),
            wrapped.ciphertext.clone().into(),
            wrapped.kdf_params.clone().into(),
            password_hash_digest.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Unlocks the user's key with the password they just proved, creating a new key when there is
/// none or when the account's password changed since the key was made (design §3). The caller
/// has already verified `password` against `password_hash` (the account's current hash).
pub async fn unlock_or_create(
    conn: &impl ConnectionTrait,
    user_id: i64,
    user_sync_id: &str,
    password: &str,
    password_hash: &str,
) -> AppResult<UnlockedSigningKey> {
    let digest = password_hash_digest(password_hash);
    if let Some(row) = current_local_key(conn, user_id).await? {
        if row.password_hash_digest == digest && row.user_sync_id == user_sync_id {
            match unwrap_key(password, user_sync_id, &row.key_id, &row.public_key, &row.wrapped) {
                Ok(key) => return Ok(key),
                Err(_) => tracing::warn!(key_id = %row.key_id, "signing key could not be unwrapped; replaced"),
            }
        }
        retire_local_keys(conn, user_id, "password_changed").await?;
    }
    let (key, wrapped) = generate_wrapped(password, user_sync_id)?;
    insert_local_key(conn, user_id, &key, &wrapped, &digest).await?;
    Ok(key)
}

#[cfg(test)]
#[path = "signing_keys_tests.rs"]
mod tests;

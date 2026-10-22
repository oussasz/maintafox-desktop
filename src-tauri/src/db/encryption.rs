//! Database encryption at rest (SQLCipher 4, AES-256).
//!
//! Key management:
//! - A random 256-bit key is generated on first run and stored in the OS credential store
//!   (Windows Credential Manager / macOS Keychain / Secret Service) via `keyring`.
//! - SQLCipher receives it as a raw key (`PRAGMA key = "x'…'"`), so no password KDF is involved.
//! - The same key, formatted for humans, is the **recovery key**: if the credential store entry is
//!   lost (OS reinstall, profile reset), the database can only be reopened with it.
//!
//! Existing plaintext databases (installs created before encryption) are encrypted once, in place,
//! with a verified copy kept until the encrypted file has been opened successfully.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rand::RngCore;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::build_identity::KEYRING_SERVICE;
use crate::errors::{AppError, AppResult};

const KEYRING_DB_KEY_ENTRY: &str = "database-encryption-key-v1";
const SQLITE_PLAINTEXT_HEADER: &[u8; 16] = b"SQLite format 3\0";
const KEY_LEN: usize = 32;

/// 256-bit database key. Deliberately not `Display`, and `Debug` is redacted, to keep it out of logs.
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseKey([u8; KEY_LEN]);

impl fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseKey(<redacted>)")
    }
}

impl DatabaseKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    fn from_hex(hex_str: &str) -> AppResult<Self> {
        let bytes = hex::decode(hex_str.trim())
            .map_err(|_| AppError::DatabaseKeyUnavailable("stored database key is not valid hex".into()))?;
        let arr: [u8; KEY_LEN] = bytes
            .try_into()
            .map_err(|_| AppError::DatabaseKeyUnavailable("stored database key has the wrong length".into()))?;
        Ok(Self(arr))
    }

    fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Value for `PRAGMA key = <value>` (SQLCipher raw-key syntax, quotes included).
    pub fn pragma_value(&self) -> String {
        format!("\"x'{}'\"", hex::encode_upper(self.0))
    }

    /// Human-readable recovery key: 64 hex chars in 16 groups of 4, e.g. `A1B2-C3D4-…`.
    pub fn recovery_string(&self) -> String {
        hex::encode_upper(self.0)
            .as_bytes()
            .chunks(4)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Parses a recovery key typed by a user (case, spaces and dashes are ignored).
    pub fn parse_recovery_string(input: &str) -> AppResult<Self> {
        let compact: String = input.chars().filter(char::is_ascii_hexdigit).collect();
        if compact.len() != KEY_LEN * 2 {
            return Err(AppError::ValidationFailed(vec![
                "La clé de récupération doit contenir 64 caractères hexadécimaux. \
                 (The recovery key must contain 64 hexadecimal characters.)"
                    .into(),
            ]));
        }
        Self::from_hex(&compact)
    }
}

/// What is on disk at the database path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseFileState {
    Missing,
    /// Standard SQLite header: created before encryption was introduced.
    Plaintext,
    /// Anything else non-empty: SQLCipher files have no recognisable header.
    Encrypted,
}

pub fn detect_file_state(path: &Path) -> AppResult<DatabaseFileState> {
    if !path.exists() {
        return Ok(DatabaseFileState::Missing);
    }
    let mut header = [0u8; 16];
    let mut file = fs::File::open(path)?;
    let read = file.read(&mut header)?;
    if read == 0 {
        // Zero-byte file (e.g. interrupted first run): nothing to protect, treat as new.
        return Ok(DatabaseFileState::Missing);
    }
    if read == header.len() && &header == SQLITE_PLAINTEXT_HEADER {
        return Ok(DatabaseFileState::Plaintext);
    }
    Ok(DatabaseFileState::Encrypted)
}

fn keyring_entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_DB_KEY_ENTRY)
        .map_err(|e| AppError::DatabaseKeyUnavailable(format!("OS credential store unavailable: {e}")))
}

/// Reads the key from the OS credential store; `Ok(None)` when no entry exists.
pub fn load_stored_key() -> AppResult<Option<DatabaseKey>> {
    match keyring_entry()?.get_password() {
        Ok(hex_key) => DatabaseKey::from_hex(&hex_key).map(Some),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::DatabaseKeyUnavailable(format!(
            "cannot read database key from OS credential store: {e}"
        ))),
    }
}

pub fn store_key(key: &DatabaseKey) -> AppResult<()> {
    keyring_entry()?
        .set_password(&key.to_hex())
        .map_err(|e| AppError::DatabaseKeyUnavailable(format!("cannot store database key in OS credential store: {e}")))
}

/// SQL-quote a filesystem path for ATTACH (no parameter binding there).
fn sql_quote_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Opens `path` with `key` and runs a read that fails if the key is wrong.
pub async fn verify_key_opens(path: &Path, key: &DatabaseKey) -> AppResult<()> {
    let conn = super::connect_sqlcipher(path, Some(key), false).await?;
    let probe = conn
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT count(*) AS n FROM sqlite_master".to_string(),
        ))
        .await;
    conn.close().await.ok();
    match probe {
        Ok(_) => Ok(()),
        Err(_) => Err(AppError::DatabaseKeyUnavailable(
            "the key does not open this database (wrong or outdated recovery key)".into(),
        )),
    }
}

/// One-time migration of a pre-encryption plaintext database to SQLCipher.
///
/// Crash-safe: the original is not touched until the encrypted copy has been verified.
/// 1. checkpoint WAL into the main file;
/// 2. `sqlcipher_export` into `<db>.encrypting` keyed with `key`;
/// 3. open the new file with the key and verify it;
/// 4. rename original → `<db>.pre-encryption.bak`, new file → `<db>`;
/// 5. remove the plaintext backup and stale `-wal`/`-shm` files only after step 4 succeeded.
pub async fn encrypt_plaintext_in_place(path: &Path, key: &DatabaseKey) -> AppResult<()> {
    let staging = sibling(path, ".encrypting");
    let backup = sibling(path, ".pre-encryption.bak");
    if staging.exists() {
        fs::remove_file(&staging)?;
    }

    {
        // ATTACH inherits the main connection's open flags: it must be allowed to create the target.
        let plain = super::connect_sqlcipher(path, None, true).await?;
        plain.execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE);").await?;
        plain
            .execute_unprepared(&format!(
                "ATTACH DATABASE '{}' AS encrypted KEY {};",
                sql_quote_path(&staging),
                key.pragma_value()
            ))
            .await?;
        plain
            .execute_unprepared("SELECT sqlcipher_export('encrypted');")
            .await?;
        plain.execute_unprepared("DETACH DATABASE encrypted;").await?;
        plain.close().await.ok();
    }

    verify_key_opens(&staging, key).await?;
    if detect_file_state(&staging)? != DatabaseFileState::Encrypted {
        fs::remove_file(&staging).ok();
        return Err(AppError::DatabaseKeyUnavailable(
            "encryption export produced a plaintext file; aborting (original untouched)".into(),
        ));
    }

    fs::rename(path, &backup)?;
    if let Err(e) = fs::rename(&staging, path) {
        // Put the original back so the app keeps working on the plaintext file.
        fs::rename(&backup, path).ok();
        return Err(e.into());
    }
    for stale in [sibling(path, "-wal"), sibling(path, "-shm")] {
        if stale.exists() {
            fs::remove_file(stale).ok();
        }
    }
    fs::remove_file(&backup)?;
    tracing::info!("database encrypted at rest (one-time migration of plaintext file)");
    Ok(())
}

/// Resolves the key for `path`, creating it on first run and encrypting a legacy plaintext file.
///
/// Errors with `DatabaseKeyUnavailable` when an encrypted database exists but the OS credential
/// store has no key: the user must supply the recovery key (never silently create a new database).
pub async fn prepare_database_key(path: &Path) -> AppResult<DatabaseKey> {
    let state = detect_file_state(path)?;
    match (state, load_stored_key()?) {
        (DatabaseFileState::Encrypted, Some(key)) => Ok(key),
        (DatabaseFileState::Encrypted, None) => Err(AppError::DatabaseKeyUnavailable(
            "the encrypted database exists but its key is missing from the OS credential store; \
             enter the recovery key"
                .into(),
        )),
        (DatabaseFileState::Missing, existing) => {
            let key = existing.unwrap_or_else(DatabaseKey::generate);
            store_key(&key)?;
            Ok(key)
        }
        (DatabaseFileState::Plaintext, existing) => {
            let key = existing.unwrap_or_else(DatabaseKey::generate);
            store_key(&key)?;
            encrypt_plaintext_in_place(path, &key).await?;
            Ok(key)
        }
    }
}

/// Stores a user-supplied recovery key after proving it opens the database at `path`.
pub async fn recover_with_recovery_key(path: &Path, recovery: &str) -> AppResult<()> {
    let key = DatabaseKey::parse_recovery_string(recovery)?;
    verify_key_opens(path, &key).await?;
    store_key(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_string_round_trips_and_ignores_formatting() {
        let key = DatabaseKey::generate();
        let shown = key.recovery_string();
        assert_eq!(shown.len(), 64 + 15, "16 groups of 4 separated by dashes");
        let parsed = DatabaseKey::parse_recovery_string(&shown.to_lowercase().replace('-', " ")).unwrap();
        assert_eq!(parsed, key);
        assert!(DatabaseKey::parse_recovery_string("ABCD-1234").is_err());
    }

    #[test]
    fn debug_output_never_contains_key_material() {
        let key = DatabaseKey::generate();
        assert_eq!(format!("{key:?}"), "DatabaseKey(<redacted>)");
    }

    #[tokio::test]
    async fn plaintext_database_is_encrypted_and_unreadable_without_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("maintafox.db");

        // Legacy plaintext database with data.
        {
            let plain = super::super::connect_sqlcipher(&path, None, true).await.unwrap();
            plain
                .execute_unprepared("CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('secret-row');")
                .await
                .unwrap();
            plain.close().await.unwrap();
        }
        assert_eq!(detect_file_state(&path).unwrap(), DatabaseFileState::Plaintext);

        let key = DatabaseKey::generate();
        encrypt_plaintext_in_place(&path, &key).await.unwrap();

        assert_eq!(detect_file_state(&path).unwrap(), DatabaseFileState::Encrypted);
        let raw = fs::read(&path).unwrap();
        assert!(
            !raw.windows(b"secret-row".len()).any(|w| w == b"secret-row"),
            "row data must not appear in the file bytes"
        );

        // Data survives with the right key…
        let enc = super::super::connect_sqlcipher(&path, Some(&key), false).await.unwrap();
        let row = enc
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT v FROM t".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "v").unwrap(), "secret-row");
        enc.close().await.unwrap();

        // …and a different key cannot open it.
        assert!(verify_key_opens(&path, &DatabaseKey::generate()).await.is_err());
        assert!(!sibling(&path, ".pre-encryption.bak").exists());
    }

    /// Backups use `VACUUM INTO`: the copy of an encrypted database must itself be encrypted with
    /// the same key (backup::run_manual_backup refuses a plaintext output).
    #[tokio::test]
    async fn vacuum_into_backup_of_encrypted_database_stays_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.db");
        let backup = dir.path().join("backup.db");
        let key = DatabaseKey::generate();

        let db = super::super::connect_sqlcipher(&live, Some(&key), true).await.unwrap();
        db.execute_unprepared("CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('backup-secret');")
            .await
            .unwrap();
        db.execute_unprepared(&format!("VACUUM INTO '{}'", sql_quote_path(&backup)))
            .await
            .unwrap();
        db.close().await.unwrap();

        assert_eq!(detect_file_state(&backup).unwrap(), DatabaseFileState::Encrypted);
        let raw = fs::read(&backup).unwrap();
        assert!(!raw.windows(b"backup-secret".len()).any(|w| w == b"backup-secret"));
        verify_key_opens(&backup, &key).await.unwrap();
        assert!(verify_key_opens(&backup, &DatabaseKey::generate()).await.is_err());
    }
}

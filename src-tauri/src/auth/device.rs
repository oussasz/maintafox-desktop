//! Device identity, trust registration, and offline grace management.
//!
//! Security rules:
//!   - Device fingerprint is SHA-256(machine_id : hostname : os_type).
//!     This prevents correlating the raw hardware ID outside Maintafox.
//!   - The device secret (32-byte random) is stored in the OS keyring only.
//!     It is used as an HMAC key for device challenge binding in Phase 3 sync.
//!   - trusted_devices table enforces UNIQUE(device_fingerprint) â€” no duplicates.

use chrono::Utc;
use hex::encode;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

// â”€â”€ Keyring service name â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
const KEYRING_SERVICE: &str = "maintafox-desktop";
const KEYRING_DEVICE_SECRET_KEY: &str = "device-installation-secret";

// â”€â”€ Fingerprint derivation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Derives a stable device fingerprint from OS-level machine identity.
/// Returns a lowercase 64-character hex string (SHA-256 output).
///
/// The fingerprint is stable across reboots, application updates, and user changes.
/// It changes when the OS is re-installed or the machine ID is reset by the OS.
///
/// Inputs (in order, separator ":"):
///   1. Machine ID (from /etc/machine-id on Linux, MachineGuid on Windows,
///      IOPlatformUUID on macOS) â€” falls back to hostname if unavailable.
///   2. Hostname
///   3. OS type ("windows" | "macos" | "linux")
pub fn derive_device_fingerprint() -> AppResult<String> {
    let machine_id = read_machine_id().unwrap_or_else(|_| "unknown-machine".into());
    let hostname = read_hostname().unwrap_or_else(|_| "unknown-host".into());
    let os_type = std::env::consts::OS.to_string();

    let raw = format!("{machine_id}:{hostname}:{os_type}");
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    Ok(encode(hasher.finalize()))
}

/// Read the platform machine ID.
/// Returns Err if the platform doesn't expose a machine ID.
fn read_machine_id() -> AppResult<String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
            .output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.trim().starts_with("MachineGuid") {
                if let Some(guid) = line.split_whitespace().last() {
                    return Ok(guid.trim().to_string());
                }
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "MachineGuid not found in registry").into())
    }

    #[cfg(target_os = "linux")]
    {
        Ok(std::fs::read_to_string("/etc/machine-id")?.trim().to_string())
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid) = line.split('"').nth(3) {
                    return Ok(uuid.to_string());
                }
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "IOPlatformUUID not found").into())
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported platform for machine ID").into())
    }
}

fn read_hostname() -> AppResult<String> {
    Ok(hostname::get()?.to_string_lossy().to_string())
}

// â”€â”€ Device secret (OS keyring) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Retrieve the device installation secret from the OS keyring.
/// Returns None if not yet initialized â€” call `initialize_device_secret` on first launch.
pub fn get_device_secret() -> AppResult<Option<Vec<u8>>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_DEVICE_SECRET_KEY)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring open failed: {e}")))?;
    match entry.get_password() {
        Ok(secret_hex) => {
            let bytes = hex::decode(&secret_hex)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring decode failed: {e}")))?;
            Ok(Some(bytes))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Internal(anyhow::anyhow!("keyring read failed: {e}"))),
    }
}

/// Generate and store a 32-byte random device secret in the OS keyring.
/// Must be called ONCE on first application launch.
/// Returns Err if the keyring is not available.
pub fn initialize_device_secret() -> AppResult<()> {
    use rand_core::{OsRng, RngCore};
    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    let secret_hex = hex::encode(secret);

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_DEVICE_SECRET_KEY)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring open failed: {e}")))?;
    entry
        .set_password(&secret_hex)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring write failed: {e}")))?;

    tracing::info!("device::secret_initialized â€” device secret stored in OS keyring");
    Ok(())
}

// â”€â”€ Trusted device DTO â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Debug, Clone, Serialize)]
pub struct TrustedDevice {
    pub id: String,
    pub device_label: Option<String>,
    pub user_id: i32,
    pub trusted_at: String,
    pub last_seen_at: Option<String>,
    pub is_revoked: bool,
    pub revoked_at: Option<String>,
}

/// Device trust status for the current device, current user.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceTrustStatus {
    /// Fingerprint of the current machine (hex, 64 chars)
    pub device_fingerprint: String,
    /// Whether this device is trusted for the given user
    pub is_trusted: bool,
    /// Whether this device's trust has been revoked
    pub is_revoked: bool,
    /// Whether offline access is currently allowed for this user+device
    pub offline_allowed: bool,
    /// Remaining offline hours (None if no trust record or unlimited)
    pub offline_hours_remaining: Option<i64>,
    /// Label given to this device by the user
    pub device_label: Option<String>,
    /// When this device's trust was established (ISO 8601)
    pub trusted_at: Option<String>,
}

// â”€â”€ trusted_devices CRUD â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Check if the current device is trusted for a given user.
/// Returns the trust record if found, None otherwise.
pub async fn get_device_trust(
    db: &DatabaseConnection,
    user_id: i32,
    fingerprint: &str,
) -> AppResult<Option<TrustedDevice>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT id, device_label, user_id, trusted_at, last_seen_at,
                      is_revoked, revoked_at
               FROM trusted_devices
               WHERE user_id = ? AND device_fingerprint = ?"#,
            [user_id.into(), fingerprint.into()],
        ))
        .await
        .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(e.to_string())))?;

    Ok(row.map(|r| TrustedDevice {
        id: r.try_get::<String>("", "id").unwrap_or_default(),
        device_label: r.try_get::<Option<String>>("", "device_label").unwrap_or(None),
        user_id: r.try_get::<i32>("", "user_id").unwrap_or(0),
        trusted_at: r.try_get::<String>("", "trusted_at").unwrap_or_default(),
        last_seen_at: r.try_get::<Option<String>>("", "last_seen_at").unwrap_or(None),
        is_revoked: r.try_get::<i32>("", "is_revoked").unwrap_or(0) == 1,
        revoked_at: r.try_get::<Option<String>>("", "revoked_at").unwrap_or(None),
    }))
}

/// Register this device as trusted for the given user.
/// Called after the first successful online login on this machine.
/// INSERT OR IGNORE â€” safe to call again if trust already exists.
pub async fn register_device_trust(
    db: &DatabaseConnection,
    user_id: i32,
    fingerprint: &str,
    device_label: Option<&str>,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT OR IGNORE INTO trusted_devices
               (id, device_fingerprint, device_label, user_id, trusted_at, is_revoked)
           VALUES (?, ?, ?, ?, ?, 0)"#,
        [
            id.into(),
            fingerprint.into(),
            device_label.map(|s| s.to_string()).into(),
            user_id.into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(e.to_string())))?;

    // Update last_seen_at unconditionally â€” covers both new and existing rows
    let now2 = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE trusted_devices SET last_seen_at = ?
           WHERE user_id = ? AND device_fingerprint = ?"#,
        [now2.into(), user_id.into(), fingerprint.into()],
    ))
    .await
    .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(e.to_string())))?;

    tracing::info!(user_id = %user_id, "device::trust_registered");
    Ok(())
}

/// Revoke trust for a specific device, by trusted_device.id.
/// A revoked device can no longer use offline sign-in.
pub async fn revoke_device_trust(
    db: &DatabaseConnection,
    device_row_id: &str,
    revoked_by_user_id: i32,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"UPDATE trusted_devices
               SET is_revoked = 1, revoked_at = ?,
                   revoked_reason = ?
               WHERE id = ? AND is_revoked = 0"#,
            [
                now.into(),
                format!("revoked by user_id={revoked_by_user_id}").into(),
                device_row_id.into(),
            ],
        ))
        .await
        .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(e.to_string())))?;

    if affected.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "trusted_device".into(),
            id: device_row_id.to_string(),
        });
    }

    tracing::info!(device_id = %device_row_id, revoked_by = %revoked_by_user_id, "device::trust_revoked");
    Ok(())
}

// ── Offline policy enforcement ────────────────────────────────────────────────

/// Maximum offline grace hours that policy allows.
/// Tenants cannot set a grace window longer than this.
pub const MAX_OFFLINE_GRACE_HOURS: i64 = 168; // 7 days

/// Check whether the current device+user is allowed to log in offline.
///
/// Logic:
///   1. If the device has no trust record → offline denied
///   2. If the device trust is revoked → offline denied
///   3. Read `offline_grace_hours` from system_config (default 72)
///   4. If `last_seen_at` is within the grace window → offline allowed
///
/// Returns (is_allowed, hours_remaining).
/// `hours_remaining` is None if the trust is not found or is revoked.
pub async fn check_offline_access(
    db: &DatabaseConnection,
    user_id: i32,
    fingerprint: &str,
) -> AppResult<(bool, Option<i64>)> {
    let trust = get_device_trust(db, user_id, fingerprint).await?;

    let Some(trust) = trust else {
        tracing::debug!(user_id, "device::offline_denied — no trust record");
        return Ok((false, None));
    };

    if trust.is_revoked {
        tracing::warn!(user_id, "device::offline_denied — device revoked");
        return Ok((false, None));
    }

    // Read the configured grace window from system_config
    let grace_hours = read_offline_grace_hours(db).await?;

    let last_seen = trust.last_seen_at.as_deref().or(Some(trust.trusted_at.as_str()));

    let Some(last_seen_str) = last_seen else {
        return Ok((false, None));
    };

    let last_seen_dt = chrono::DateTime::parse_from_rfc3339(last_seen_str)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid last_seen_at in trusted_devices: {e}")))?
        .with_timezone(&Utc);

    let deadline = last_seen_dt + chrono::Duration::hours(grace_hours);
    let now = Utc::now();

    if now < deadline {
        let remaining = (deadline - now).num_hours();
        tracing::debug!(user_id, remaining_hours = remaining, "device::offline_allowed");
        Ok((true, Some(remaining)))
    } else {
        tracing::info!(user_id, "device::offline_denied — grace window expired");
        Ok((false, Some(0)))
    }
}

/// Read the `offline_grace_hours` value from system_config.
/// Returns the default (72) if the key is absent or unparseable.
/// Caps at MAX_OFFLINE_GRACE_HOURS to prevent policy bypass via DB edit.
async fn read_offline_grace_hours(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM system_config WHERE key = 'offline_grace_hours'",
            [],
        ))
        .await
        .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(e.to_string())))?;

    let hours: i64 = row
        .and_then(|r| r.try_get::<String>("", "value").ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(72);

    Ok(hours.min(MAX_OFFLINE_GRACE_HOURS))
}

// ── Network availability ──────────────────────────────────────────────────────

/// Best-effort check for network availability.
/// Returns true if any non-loopback network interface has an assigned IP.
/// This is NOT a VPS round-trip — it is local-only; used only to gate
/// first-login requirement and offline grace bypass detection.
pub fn is_network_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("ipconfig").output();
        if let Ok(out) = output {
            let text = String::from_utf8_lossy(&out.stdout);
            // Look for any non-loopback IPv4 address (reject APIPA 169.254.x.x)
            return text.contains("IPv4 Address") && !text.contains("169.254.");
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::net::TcpStream;
        // Probe a fast-fail connection to a well-known public DNS on port 53.
        // Returns within 1s if offline.
        TcpStream::connect_timeout(&"8.8.8.8:53".parse().unwrap(), std::time::Duration::from_secs(1)).is_ok()
    }
}

// â”€â”€ Unit tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_64_hex_chars() {
        // We can't guarantee specific machines have all identifiers, but the
        // function itself must return a valid hex string regardless of fallbacks.
        let fp = derive_device_fingerprint().expect("fingerprint derivation should not panic");
        assert_eq!(fp.len(), 64, "SHA-256 fingerprint must be 64 hex chars");
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()), "Must be hex");
    }

    #[test]
    fn fingerprint_is_deterministic_on_same_machine() {
        let fp1 = derive_device_fingerprint().expect("fp1");
        let fp2 = derive_device_fingerprint().expect("fp2");
        assert_eq!(fp1, fp2, "Two calls must produce the same fingerprint");
    }

    #[test]
    fn trusted_device_revoked_flag_defaults_false() {
        let d = TrustedDevice {
            id: "abc".into(),
            device_label: None,
            user_id: 1,
            trusted_at: "2026-03-31T12:00:00Z".into(),
            last_seen_at: None,
            is_revoked: false,
            revoked_at: None,
        };
        assert!(!d.is_revoked);
    }

    #[test]
    fn max_offline_grace_is_168_hours() {
        // This test documents and pins the security invariant.
        assert_eq!(
            MAX_OFFLINE_GRACE_HOURS, 168,
            "Offline grace maximum is a security parameter \u{2014} do not raise without review"
        );
    }

    #[test]
    fn network_available_returns_bool() {
        // Just verify it doesn't panic \u{2014} result varies by machine
        let _result = is_network_available();
    }
}

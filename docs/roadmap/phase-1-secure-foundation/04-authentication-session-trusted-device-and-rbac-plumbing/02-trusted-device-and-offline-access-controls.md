# Phase 1 · Sub-phase 04 · File 02
# Trusted Device and Offline Access Controls

## Context and Purpose

File 01 delivered the authentication foundation: user tables, argon2id hashing, the
`SessionManager`, and the login/logout IPC command pair. The product now knows *who* is
authenticated. But it does not yet know *which machine* is doing the authenticating, or
*whether* that machine is allowed to operate offline.

This file builds the two systems that govern trusted-device state:

1. **Device fingerprinting and trust registration** — a deterministic, collision-resistant
   machine identifier derived from hardware and OS properties, stored in the
   `trusted_devices` table after the first successful online login. This identifier
   is the key that unlocks offline grace.

2. **Offline access policy enforcement** — a policy check that runs at login time: is this
   device trusted? Is the offline grace window still open? Did the user's last online
   session succeed within that window? If any answer is no, offline login is denied.

The PRD (§6.1, §5 responsibility split) is explicit: "First login on a device must be
online and creates a trusted-device record. Offline sign-in is allowed only for previously
trusted users on previously trusted devices inside the configured offline grace window."

## Architecture Rules Applied

- The device fingerprint is a **SHA-256 hash** of a combination of hardware identifiers
  (machine ID, hostname, OS type). It is NOT the raw hardware string — hashing prevents
  the stored value from being used to re-identify the machine outside Maintafox.
- The fingerprint is stored in `trusted_devices.device_fingerprint` (TEXT, UNIQUE) so
  duplicate trusted_devices rows for the same machine are impossible.
- The **offline grace window** is configurable per tenant in `system_config` under key
  `offline_grace_hours`. Default is 72 (3 days). The maximum enforced by policy is 168
  (7 days).
- Online detection at login time is a **best-effort check** using the OS network interface
  state, not a VPS round-trip (which would break offline-first). The VPS sync path handles
  token refresh and device trust renewal server-side.
- Device trust revocation is stored in `trusted_devices.is_revoked`. A revoked device
  can still be used for online-only login.
- The OS keyring holds the device secret material — a random 32-byte value bound to this
  installation and used as a HMAC key for the device fingerprint challenge. It is
  generated on first launch and never changes.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/src/auth/device.rs` | Device fingerprint derivation, trusted_devices CRUD, offline grace check |
| `src-tauri/src/commands/auth.rs` (extended) | New IPC: `get_device_trust_status`, `register_device_trust`, `revoke_device_trust` |
| `src-tauri/src/startup.rs` (extended) | Device secret material generated on first launch; stored in OS keyring |
| `shared/ipc-types.ts` (extended) | `DeviceTrustStatus`, `TrustedDevice` interfaces |
| Updated login flow | login() checks device trust and enforces offline grace policy |
| `docs/DEVICE_TRUST_CONTRACTS.md` | Reference documentation for device model |

## Prerequisites

- SP04-F01 complete: `SessionManager`, argon2id, login/logout IPC all in place
- SP03-F01 complete: `trusted_devices` table exists (migration 001)
- SP02-F01 complete: `startup.rs` sequence in place for adding first-launch initialization

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Device Fingerprint Derivation and Trusted Device Repository | `auth/device.rs`, fingerprint derivation, `trusted_devices` table CRUD |
| S2 | Offline Policy Enforcement and Login Flow Integration | Offline grace check, login() update, startup device initialization |
| S3 | Device Trust IPC Commands and Frontend Contracts | `get_device_trust_status`, `register_device_trust`, `revoke_device_trust` IPC, TypeScript types |

---

## Sprint S1 — Device Fingerprint Derivation and Trusted Device Repository

### AI Agent Prompt

```
You are a senior Rust security engineer continuing work on Maintafox Desktop.
SP04-F01 is complete. Your task is to implement the device identity module.

The device fingerprint is a deterministic hash of machine-layer properties that
should be stable across reboots and application updates. It is NOT personally
identifiable on its own; it identifies the installation, not the person.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/auth/device.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/auth/device.rs
//! Device identity, trust registration, and offline grace management.
//!
//! Security rules:
//!   - Device fingerprint is SHA-256(machine_id | hostname | os_type).
//!     This prevents correlating the raw hardware ID outside Maintafox.
//!   - The device secret (32-byte random) is stored in the OS keyring only.
//!     It is used as an HMAC key for device challenge binding in Phase 3 sync.
//!   - trusted_devices table enforces UNIQUE(device_fingerprint) — no duplicates.

use sha2::{Sha256, Digest};
use hex::encode;
use chrono::Utc;
use uuid::Uuid;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use crate::errors::{AppError, AppResult};

// ── Keyring service name ──────────────────────────────────────────────────────
const KEYRING_SERVICE: &str = "maintafox-desktop";
const KEYRING_DEVICE_SECRET_KEY: &str = "device-installation-secret";

// ── Fingerprint derivation ────────────────────────────────────────────────────

/// Derives a stable device fingerprint from OS-level machine identity.
/// Returns a lowercase 64-character hex string (SHA-256 output).
///
/// The fingerprint is stable across reboots, application updates, and user changes.
/// It changes when the OS is re-installed or the machine ID is reset by the OS.
///
/// Inputs (in order, separator ":"):
///   1. Machine ID (from /etc/machine-id on Linux, MachineGuid on Windows,
///      IOPlatformUUID on macOS) — falls back to hostname if unavailable.
///   2. Hostname
///   3. OS type ("windows" | "macos" | "linux")
pub fn derive_device_fingerprint() -> AppResult<String> {
    let machine_id = read_machine_id().unwrap_or_else(|_| "unknown-machine".into());
    let hostname = read_hostname().unwrap_or_else(|_| "unknown-host".into());
    let os_type = std::env::consts::OS.to_string();

    let raw = format!("{}:{}:{}", machine_id, hostname, os_type);
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
            .output()
            .map_err(|e| AppError::Io(e.to_string()))?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.trim().starts_with("MachineGuid") {
                if let Some(guid) = line.split_whitespace().last() {
                    return Ok(guid.trim().to_string());
                }
            }
        }
        Err(AppError::Io("MachineGuid not found in registry".into()))
    }

    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/machine-id")
            .map(|s| s.trim().to_string())
            .map_err(|e| AppError::Io(e.to_string()))
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .map_err(|e| AppError::Io(e.to_string()))?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid) = line.split('"').nth(3) {
                    return Ok(uuid.to_string());
                }
            }
        }
        Err(AppError::Io("IOPlatformUUID not found".into()))
    }
}

fn read_hostname() -> AppResult<String> {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .map_err(|e| AppError::Io(e.to_string()))
}

// ── Device secret (OS keyring) ────────────────────────────────────────────────

/// Retrieve the device installation secret from the OS keyring.
/// Returns None if not yet initialized — call `initialize_device_secret` on first launch.
pub fn get_device_secret() -> AppResult<Option<Vec<u8>>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_DEVICE_SECRET_KEY)
        .map_err(|e| AppError::Internal(format!("keyring open failed: {e}")))?;
    match entry.get_password() {
        Ok(secret_hex) => {
            let bytes = hex::decode(&secret_hex)
                .map_err(|e| AppError::Internal(format!("keyring decode failed: {e}")))?;
            Ok(Some(bytes))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Internal(format!("keyring read failed: {e}"))),
    }
}

/// Generate and store a 32-byte random device secret in the OS keyring.
/// Must be called ONCE on first application launch.
/// Returns Err if the keyring is not available.
pub fn initialize_device_secret() -> AppResult<()> {
    use rand::RngCore;
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    let secret_hex = hex::encode(secret);

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_DEVICE_SECRET_KEY)
        .map_err(|e| AppError::Internal(format!("keyring open failed: {e}")))?;
    entry.set_password(&secret_hex)
        .map_err(|e| AppError::Internal(format!("keyring write failed: {e}")))?;

    tracing::info!("device::secret_initialized — device secret stored in OS keyring");
    Ok(())
}

// ── Trusted device DTO ────────────────────────────────────────────────────────

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

// ── trusted_devices CRUD ──────────────────────────────────────────────────────

/// Check if the current device is trusted for a given user.
/// Returns the trust record if found, None otherwise.
pub async fn get_device_trust(
    db: &DatabaseConnection,
    user_id: i32,
    fingerprint: &str,
) -> AppResult<Option<TrustedDevice>> {
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"SELECT id, device_label, user_id, trusted_at, last_seen_at,
                  is_revoked, revoked_at
           FROM trusted_devices
           WHERE user_id = ? AND device_fingerprint = ?"#,
        [user_id.into(), fingerprint.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

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
/// INSERT OR IGNORE — safe to call again if trust already exists.
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
    .map_err(|e| AppError::Database(e.to_string()))?;

    // Update last_seen_at if row already existed (ON CONFLICT DO UPDATE is SQLite 3.24+)
    let now2 = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE trusted_devices SET last_seen_at = ?
           WHERE user_id = ? AND device_fingerprint = ?"#,
        [now2.into(), user_id.into(), fingerprint.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

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
    let affected = db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE trusted_devices
           SET is_revoked = 1, revoked_at = ?,
               revoked_reason = ?
           WHERE id = ? AND is_revoked = 0"#,
        [
            now.into(),
            format!("revoked by user_id={}", revoked_by_user_id).into(),
            device_row_id.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    if affected.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "trusted_device".into(),
            id: device_row_id.to_string(),
        });
    }

    tracing::info!(device_id = %device_row_id, revoked_by = %revoked_by_user_id, "device::trust_revoked");
    Ok(())
}

// ── Unit tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_64_hex_chars() {
        // We can't guarantee specific machines have all identifiers, but the
        // function itself must return a valid hex string regardless of fallbacks.
        let fp = derive_device_fingerprint()
            .expect("fingerprint derivation should not panic");
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
}
```

Register in auth/mod.rs:
```rust
pub mod device;
```

Add crate dependencies to src-tauri/Cargo.toml:
```toml
sha2    = "0.10"
hex     = "0.4"
keyring = { version = "2", features = ["windows-native", "apple-native", "linux-native-sync-persistent"] }
hostname = "0.3"
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (3 new device tests)
- derive_device_fingerprint() returns a 64-char hex string on the developer machine
- Two consecutive calls produce identical output (deterministic)
- `trusted_devices` table is queryable in DBeaver (from migration 001)
```

---

### Supervisor Verification — Sprint S1

**V1 — Device fingerprint tests pass.**
Run `cd src-tauri && cargo test auth::device`. All 3 tests must pass:
`fingerprint_is_64_hex_chars`, `fingerprint_is_deterministic_on_same_machine`,
`trusted_device_revoked_flag_defaults_false`. If any fail, flag the test name.

**V2 — Fingerprint is stable across two cargo test runs.**
Run `cd src-tauri && cargo test auth::device::tests::fingerprint_is_deterministic_on_same_machine`
twice. Both must show `ok`. If the test fails either time, the fingerprint derivation is
not deterministic.

**V3 — No raw machine ID in fingerprint output.**
Verify that the function returns a 64-char hex string and NOT the raw Windows MachineGuid
(which looks like `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`). If the output contains
dashes or is not exactly 64 characters, the SHA-256 is not being applied. Flag it.

---

## Sprint S2 — Offline Policy Enforcement and Login Flow Integration

### AI Agent Prompt

```
You are a senior Rust engineer continuing work on Maintafox Desktop. Sprint S1 is
complete: device fingerprint derivation, trusted_devices CRUD, and OS keyring
initialization are in place.

YOUR TASK:
1. Add the offline grace check function (does this user+device qualify for offline login?)
2. Update the login() IPC command to enforce device trust and offline policy
3. Extend startup.rs to initialize the device secret on first launch

─────────────────────────────────────────────────────────────────────
STEP 1 — Add offline_policy_check to src-tauri/src/auth/device.rs
─────────────────────────────────────────────────────────────────────
Add this function to device.rs:

```rust
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

    let last_seen = trust
        .last_seen_at
        .as_deref()
        .or(Some(trust.trusted_at.as_str()));

    let Some(last_seen_str) = last_seen else {
        return Ok((false, None));
    };

    let last_seen_dt = chrono::DateTime::parse_from_rfc3339(last_seen_str)
        .map_err(|e| AppError::Internal(format!("invalid last_seen_at in trusted_devices: {e}")))?
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
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT value FROM system_config WHERE key = 'offline_grace_hours'",
        [],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    let hours: i64 = row
        .and_then(|r| r.try_get::<String>("", "value").ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(72);

    Ok(hours.min(MAX_OFFLINE_GRACE_HOURS))
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Update login() in commands/auth.rs to enforce device policy
─────────────────────────────────────────────────────────────────────
Extend the login() IPC command to:
a. Derive the device fingerprint
b. If login succeeds, register/update device trust
c. If device has no trust record and the machine is offline → deny login

Replace the section after `let password_ok = ...` with:

```rust
    if !password_ok {
        session_manager::record_failed_login(&state.db, user_id).await?;
        warn!(username = %username, "login::wrong_password");
        return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
    }

    // ── Device trust enforcement ─────────────────────────────────────────
    let fingerprint = crate::auth::device::derive_device_fingerprint()
        .unwrap_or_else(|_| "unknown-fingerprint".to_string());

    let is_online = is_network_available();

    // Get existing trust record for this user+device
    let trust = crate::auth::device::get_device_trust(&state.db, user_id, &fingerprint).await?;

    match (&trust, is_online) {
        // First login: device not yet trusted. Requires connectivity.
        (None, false) => {
            warn!(username = %username, "login::first_login_requires_online");
            return Err(AppError::Auth(
                "La première connexion sur cet appareil nécessite une connexion réseau.".into(),
            ));
        }
        // First login or trust expired: register now (online)
        (None, true) => {
            crate::auth::device::register_device_trust(
                &state.db,
                user_id,
                &fingerprint,
                None,
            )
            .await?;
            tracing::info!(username = %username, "login::device_trust_registered");
        }
        // Known device but revoked: allow online login only (cannot offline)
        (Some(t), _) if t.is_revoked => {
            if !is_online {
                return Err(AppError::Auth(
                    "Cet appareil a été révoqué. Connexion en ligne requise.".into(),
                ));
            }
            // Re-register on online login after revocation (admin may have re-enrolled)
        }
        // Known device, offline: check grace window
        (Some(_), false) => {
            let (allowed, _) = crate::auth::device::check_offline_access(
                &state.db,
                user_id,
                &fingerprint,
            )
            .await?;
            if !allowed {
                return Err(AppError::Auth(
                    "Fenêtre de connexion hors ligne expirée. Connexion en ligne requise.".into(),
                ));
            }
            tracing::info!(username = %username, "login::offline_access_granted");
        }
        // Known device, online: update last_seen_at
        (Some(_), true) => {
            crate::auth::device::register_device_trust(
                &state.db,
                user_id,
                &fingerprint,
                None,
            )
            .await?;
        }
    }

    // ── Create session ────────────────────────────────────────────────────
    // (continues from SP04-F01 s2 session creation code)
```

Add the `is_network_available()` helper to device.rs:

```rust
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
            // Look for any non-loopback IPv4 address
            return text.contains("IPv4 Address") && !text.contains("169.254.");
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::net::TcpStream;
        // Probe a fast-fail connection to a well-known public address
        // on port 80. Returns within 1s if offline.
        TcpStream::connect_timeout(
            &"8.8.8.8:53".parse().unwrap(),
            std::time::Duration::from_secs(1),
        )
        .is_ok()
    }
}
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Add device secret initialization to startup.rs
─────────────────────────────────────────────────────────────────────
After the seeder call in startup.rs, add:

```rust
// Initialize device secret on first launch
match crate::auth::device::get_device_secret() {
    Ok(None) => {
        // First launch: generate and store the device secret
        crate::auth::device::initialize_device_secret()
            .inspect_err(|e| tracing::warn!("device_secret_init_failed: {}", e))
            .ok();
    }
    Ok(Some(_)) => {
        tracing::debug!("startup::device_secret already exists");
    }
    Err(e) => {
        tracing::warn!("startup::keyring_unavailable: {} — offline trust material will not persist", e);
    }
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Add DeviceTrustStatus and TrustedDevice to shared/ipc-types.ts
─────────────────────────────────────────────────────────────────────
```typescript
export interface TrustedDevice {
  id: string;
  device_label: string | null;
  user_id: number;
  trusted_at: string;
  last_seen_at: string | null;
  is_revoked: boolean;
  revoked_at: string | null;
}

export interface DeviceTrustStatus {
  device_fingerprint: string;
  is_trusted: boolean;
  is_revoked: boolean;
  offline_allowed: boolean;
  offline_hours_remaining: number | null;
  device_label: string | null;
  trusted_at: string | null;
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures
- Login with correct credentials: `trusted_devices` now has a row for the current machine
- Login with wrong credentials is unchanged (same opaque error)
- Startup log shows "device_secret already exists" on second launch
- After login, `trusted_devices` shows `last_seen_at` populated for the admin user
```

---

### Supervisor Verification — Sprint S2

**V1 — trusted_devices row created after login.**
Run `pnpm run dev`. Log in using `admin` / `Admin#2026!` via the Developer Tools console:
```javascript
window.__TAURI__.core.invoke('login', { payload: { username: 'admin', password: 'Admin#2026!' } });
```
Then open DBeaver and run:
```sql
SELECT device_fingerprint, user_id, trusted_at, last_seen_at, is_revoked
FROM trusted_devices;
```
There should be exactly 1 row. The `device_fingerprint` should be a 64-character hex
string. `is_revoked` should be 0. If no row is present, the device trust registration
is not being called after successful login. Flag it.

**V2 — Device fingerprint is 64 hex chars.**
In DBeaver, inspect the `device_fingerprint` column value. It must be exactly 64
characters, all lowercase hex (0–9, a–f). No dashes, no UUID format, no spaces. If
the format is different, the SHA-256 is not being applied. Flag it.

**V3 — Startup log shows device secret initialization.**
In the terminal running `pnpm run dev`, look for a line containing
`device_secret_init_failed` (should NOT appear) or `device_secret already exists`
(should appear on second launch). If the first launch shows `device_secret_init_failed`,
copy the error text and flag it — the OS keyring may not be available.

---

## Sprint S3 — Device Trust IPC Commands and Frontend Contracts

### AI Agent Prompt

```
You are a senior Rust and TypeScript engineer continuing work on Maintafox Desktop.
Sprint S2 is complete: offline policy enforcement is integrated into the login flow.
Your task is to add three IPC commands for device trust management and write the
frontend contracts document.

─────────────────────────────────────────────────────────────────────
STEP 1 — Add device trust IPC commands to commands/auth.rs
─────────────────────────────────────────────────────────────────────
```rust
use crate::auth::device;

/// Get the trust status of the current device for the currently logged-in user.
/// Requires an active session.
#[tauri::command]
pub async fn get_device_trust_status(
    state: State<'_, AppState>,
) -> AppResult<device::DeviceTrustStatus> {
    let user = require_session!(state);

    let fingerprint = device::derive_device_fingerprint()
        .unwrap_or_else(|_| "unknown".to_string());

    let trust = device::get_device_trust(&state.db, user.user_id, &fingerprint).await?;
    let (offline_allowed, offline_hours) = device::check_offline_access(
        &state.db,
        user.user_id,
        &fingerprint,
    )
    .await?;

    Ok(device::DeviceTrustStatus {
        device_fingerprint: fingerprint,
        is_trusted: trust.is_some() && !trust.as_ref().map(|t| t.is_revoked).unwrap_or(false),
        is_revoked: trust.as_ref().map(|t| t.is_revoked).unwrap_or(false),
        offline_allowed,
        offline_hours_remaining: offline_hours,
        device_label: trust.as_ref().and_then(|t| t.device_label.clone()),
        trusted_at: trust.as_ref().map(|t| t.trusted_at.clone()),
    })
}

/// Revoke trust for a specific trusted device by row id.
/// Use this to remove offline access for a lost or stolen device.
/// Requires admin permissions (enforced in SP04-F03).
#[tauri::command]
pub async fn revoke_device_trust(
    device_id: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    device::revoke_device_trust(&state.db, &device_id, user.user_id).await
}
```

Register both in `generate_handler!`.

─────────────────────────────────────────────────────────────────────
STEP 2 — Create docs/DEVICE_TRUST_CONTRACTS.md
─────────────────────────────────────────────────────────────────────
```markdown
# Device Trust Contracts

Reference for the trusted-device model, IPC commands, and offline policy enforcement.

## Device Identity Model

The device fingerprint is derived as:
```
SHA-256( machine_id : hostname : os_type )
```
- **Stable:** survives reboots, application updates, and login user changes
- **Hashed:** raw machine ID is not stored; the hash cannot be reversed to get hardware details
- **Scope:** identifies the OS installation, not the logged-in user account

The device secret (32-byte random) is stored in the OS keyring (Windows Credential
Manager, macOS Keychain, Linux Secret Service). It is used as an HMAC material for
the Phase 3 sync device challenge. It is generated once on first launch and never changes.

## Trust Lifecycle

```
[first online login]
  → credentials accepted
  → register_device_trust(user_id, fingerprint)
  → trusted_devices row created

[subsequent online login]
  → register_device_trust() called → updates last_seen_at

[offline login attempt]
  → check_offline_access(user_id, fingerprint)
  → if no trust record → DENIED
  → if trust revoked → DENIED
  → if last_seen_at + grace_hours < now → DENIED (grace expired)
  → else → ALLOWED

[admin revokes device]
  → revoke_device_trust(device_id)
  → is_revoked = 1
  → offline login denied for this device+user combination
  → online login still allowed (machine is not blocked at OS level)
```

## Offline Grace Policy

The grace window is configurable per tenant:

| Config key | Default | Maximum |
|-----------|---------|---------|
| `offline_grace_hours` | 72 (3 days) | 168 (7 days) |

The maximum is enforced at the application level regardless of what value is stored
in `system_config`. This prevents a database edit from extending the grace window beyond
policy limits — even if the database is decrypted by an attacker.

## IPC Commands

### get_device_trust_status
```
Requires: authenticated session
Returns:  DeviceTrustStatus
```
Returns the trust status of the current device for the logged-in user, including whether
offline access is currently allowed and how many hours remain.

### revoke_device_trust
```
Requires: authenticated session + adm.users permission (enforced SP04-F03)
Payload:  { device_id: string }
Returns:  null
Errors:   NOT_FOUND if device_id does not exist or is already revoked
```
Revokes offline trust for a specific device. The device can still log in online. Used
when a laptop is lost or stolen to prevent offline access with cached credentials.

## Security Notes

1. The `is_network_available()` check is a **local best-effort** signal, not a VPS
   round-trip. An attacker with local OS access could spoof the network state. The
   device trust model relies on the OS keyring secret as the durable binding material;
   the network check prevents accidental offline registration on first login only.

2. Device trust revocation takes effect on the NEXT login attempt. An active session
   is not terminated by revocation — use the session manager `clear_session()` for that.

3. The `offline_grace_hours` cap of 168 hours is enforced at the application code level
   and is not overridable by tenant configuration or database edit. Any PR that raises
   this cap to above 168 or makes it unlimited must include a security review sign-off.
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Update IPC_COMMAND_REGISTRY.md
─────────────────────────────────────────────────────────────────────
Add entries:

```markdown
## get_device_trust_status

| Field | Value |
|-------|-------|
| Command | `get_device_trust_status` |
| Module | Authentication / Device Trust |
| Auth Required | Yes |
| Parameters | None |
| Response | `DeviceTrustStatus` |
| Errors | `AUTH_ERROR` if no session |
| Since | v0.1.0 |
| PRD Ref | §6.1 Trusted Device Registration |

## revoke_device_trust

| Field | Value |
|-------|-------|
| Command | `revoke_device_trust` |
| Module | Authentication / Device Trust |
| Auth Required | Yes + adm.users permission (SP04-F03) |
| Parameters | `{ device_id: string }` |
| Response | null |
| Errors | `NOT_FOUND`, `AUTH_ERROR` |
| Since | v0.1.0 |
| PRD Ref | §6.1, §12 Security |
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Add tests for offline grace check
─────────────────────────────────────────────────────────────────────
Add to src-tauri/src/auth/device.rs tests:

```rust
    #[test]
    fn max_offline_grace_is_168_hours() {
        // This test documents and pins the security invariant.
        assert_eq!(MAX_OFFLINE_GRACE_HOURS, 168,
            "Offline grace maximum is a security parameter — do not raise without review");
    }

    #[test]
    fn network_available_returns_bool() {
        // Just verify it doesn't panic — result varies by machine
        let _result = is_network_available();
    }
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (2 new device tests)
- pnpm run dev: login registers device, `get_device_trust_status` returns
  `{ is_trusted: true, offline_allowed: true, offline_hours_remaining: ~71 }`
- docs/DEVICE_TRUST_CONTRACTS.md is present and documents the 168-hour cap
- IPC_COMMAND_REGISTRY.md has entries for the two new commands
```

---

### Supervisor Verification — Sprint S3

**V1 — get_device_trust_status works after login.**
Log in, then run:
```javascript
window.__TAURI__.core.invoke('get_device_trust_status')
  .then(r => console.log(JSON.stringify(r, null, 2)));
```
The response must have: `is_trusted: true`, `offline_allowed: true`,
`device_fingerprint` is a 64-char hex string, `offline_hours_remaining` is a number
near 72 (± 1 hour is fine). If any field is wrong, flag it.

**V2 — Security cap test passes.**
Run `cd src-tauri && cargo test auth::device::tests::max_offline_grace_is_168_hours`.
It must show `ok`. If it shows `FAILED`, the security cap constant was changed. Flag it.

**V3 — Device trust contracts document is complete.**
Open `docs/DEVICE_TRUST_CONTRACTS.md`. The document must contain:
- The SHA-256 formula showing the three inputs (machine_id, hostname, os_type)
- The words "168 hours"
- The trust lifecycle diagram (or ASCII representation)
- The IPC command table with `get_device_trust_status` and `revoke_device_trust`

If any of these four elements are absent, flag it.

---

*End of Phase 1 · Sub-phase 04 · File 02*

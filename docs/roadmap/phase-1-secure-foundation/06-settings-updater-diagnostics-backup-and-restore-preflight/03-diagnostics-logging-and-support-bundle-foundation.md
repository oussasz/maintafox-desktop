# Phase 1 · Sub-phase 06 · File 03
# Diagnostics, Logging, and Support Bundle Foundation

## Context and Purpose

Files 01 and 02 delivered the settings control plane and the in-app updater skeleton.
This file addresses the observability half of the operational foundation: structured
application logging, persistent log file rotation, and the ability to generate a
**support bundle** — a one-click diagnostic artifact that an administrator can share
with the support team when something goes wrong.

The support bundle is specified in PRD §6.20 (In-App Documentation and Support Center):
it captures product version, OS information, database schema version, sync status, active
locale, and the last N sanitized log lines. It is a JSON blob that can be copied to the
clipboard or saved to a file.

Phase 1 does not require a ticketing integration — that is a Phase 2 feature. But the
infrastructure to generate the bundle must be in place and tested before Phase 1 closes.
In particular, the **log sanitization** constraint is strict: no password, token, secret
handle, or key material may appear in any log line that is included in a support bundle.
This is enforced by the log sanitizer in the diagnostics module.

Structured logging also benefits the entire existing codebase: from this point forward,
every `tracing::info!`, `tracing::warn!`, and `tracing::error!` call is written to a
rolling log file in addition to the development console. This gives operators a complete
history of events without relying on a connected debugger.

## Architecture Rules Applied

- **`tracing` ecosystem.** The existing `tracing` crate (from SP01-F02) provides the
  instrumentation API. This file adds `tracing-appender` for file-based output and
  confirms the subscriber is configured for both console (dev) and file (prod/dev).
- **Rolling log files.** Logs rotate daily. Seven days of logs are retained. On typical
  production hardware this means at most ~50 MB of log files.
- **Log sanitizer.** Before any log line is included in a support bundle, it is passed
  through a regex-based sanitizer that redacts: passwords, tokens, bearer headers, key
  handles, and any value that looks like a secret (64-char hex, base64 > 64 chars).
- **Support bundle is read-only.** The bundle is a snapshot — it does not modify any
  state, does not trigger any network calls, and does not open any files except the
  log files and a few OS information APIs.
- **No PII in logs.** The structured logging discipline from SP04-F01 requires that user
  IDs (not names) are logged. This file does not change that constraint — the sanitizer
  is a second line of defense, not a replacement for disciplined log hygiene.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/Cargo.toml` (patch) | Add `tracing-appender`, `sysinfo`, `regex`; add `"json"` feature to `tracing-subscriber` |
| `src-tauri/src/diagnostics/mod.rs` | File-based logging setup, support bundle generation, sanitizer, inline `#[cfg(test)]` unit tests |
| `src-tauri/src/commands/diagnostics.rs` | IPC: `get_diagnostics_info`, `generate_support_bundle` |
| `src-tauri/src/lib.rs` (patch) | Register diagnostics commands; init file-based logging |
| `shared/ipc-types.ts` (patch) | Add `DiagnosticsAppInfo` and `SupportBundle` interfaces |
| `src/services/diagnostics-service.ts` (patch) | Frontend IPC wrappers (`getDiagnosticsInfo`, `generateSupportBundle`) |
| `src/components/SupportBundleDialog.tsx` | Dialog stub: shows app info + copy-to-clipboard |
| `src/i18n/locale-data/{en,fr}/diagnostics.json` | Translation keys for the diagnostics namespace |
| `src/i18n/namespaces.ts` (patch) | Register `diagnostics` in `MODULE_NAMESPACES` |
| `src/i18n/types.ts` (patch) | Add `diagnostics` to `CustomTypeOptions.resources` |
| `scripts/check-i18n-parity.ts` (patch) | Add `diagnostics` to its hardcoded namespace list |

## Prerequisites

- SP02-F01: `tracing` subscriber initialized in `setup.rs`
- SP06-F01: `app_settings` table present, `settings::get_setting()` available
- SP04-F01: session management present (`require_session!` available)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | File-Based Logging and Support Bundle Rust Module | `diagnostics/mod.rs`, Cargo.toml patch, lib.rs logging init |
| S2 | Diagnostics IPC Commands and Sanitizer Tests | `commands/diagnostics.rs`, sanitizer unit tests |
| S3 | Frontend Dialog Stub and Service | `diagnostics-service.ts`, `SupportBundleDialog.tsx` |

---

## Sprint S1 — File-Based Logging and Support Bundle Rust Module

### AI Agent Prompt

```
You are a senior Rust engineer. The existing tracing subscriber from SP02-F01 writes to
the console only. Your task is to add file-based log rotation and the core support bundle
generation logic.

────────────────────────────────────────────────────────────────────
STEP 1 — PATCH src-tauri/Cargo.toml
────────────────────────────────────────────────────────────────────
Add to [dependencies]:
```toml
tracing-appender = "0.2"
sysinfo = "0.30"
regex = "1"
```

Also add the `"json"` feature to the existing `tracing-subscriber` dependency
so the file layer can emit machine-readable JSON.

`tracing-appender` provides file-based, non-blocking log rotation.
`sysinfo` provides OS name, version, available memory, and architecture.
`regex` is used by the log sanitizer.

────────────────────────────────────────────────────────────────────
STEP 2 — CREATE src-tauri/src/diagnostics/mod.rs
────────────────────────────────────────────────────────────────────
```rust
//! Diagnostics and support bundle module.
//!
//! Responsibilities:
//! 1. Initialize file-based rolling log output (called once from lib.rs setup)
//! 2. Expose `DiagnosticsAppInfo` and `SupportBundle` types
//! 3. Generate support bundles from log files + OS info + DB state
//! 4. Sanitize log lines before including them in a bundle
//!
//! This module is the core logic layer. IPC command wrappers live in
//! `commands::diagnostics` and are session-gated.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tracing_appender::rolling;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Rich application metadata returned by `get_diagnostics_info`.
///
/// Richer than the pre-auth `AppInfoResponse` from `commands::app` —
/// includes DB schema version, active locale from settings, and uptime.
/// Requires an active authenticated session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsAppInfo {
    /// Semantic version from Cargo.toml, e.g. "1.0.0-dev"
    pub app_version: String,
    /// Operating system name, e.g. "Windows" | "macOS" | "Linux"
    pub os_name: String,
    /// OS version string, e.g. "Windows 11 23H2"
    pub os_version: String,
    /// CPU architecture, e.g. "x86_64" | "aarch64"
    pub arch: String,
    /// Number of applied database migrations
    pub db_schema_version: i64,
    /// Active locale code, e.g. "fr-CA"
    pub active_locale: String,
    /// Simple sync status label (not a detailed audit — Phase 2 will expand this)
    pub sync_status: String,
    /// Seconds since the application process started
    pub uptime_seconds: u64,
}

/// Support bundle — all fields are safe to share with the support team.
/// Log lines are sanitized before inclusion (no secrets, tokens, or keys).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportBundle {
    /// ISO 8601 timestamp when the bundle was generated
    pub generated_at: String,
    pub app_info: DiagnosticsAppInfo,
    /// Up to 500 sanitized log lines, oldest first
    pub log_lines: Vec<String>,
    /// Any errors encountered while collecting bundle data
    pub collection_warnings: Vec<String>,
}

// ─── Application start time ───────────────────────────────────────────────────
// Stored as a process-lifetime OnceLock so uptime is available anywhere without
// holding a handle.

static APP_START: OnceLock<Instant> = OnceLock::new();

/// Must be called once at application startup (before the Tauri builder runs).
pub fn record_start_time() {
    APP_START.get_or_init(Instant::now);
}

pub fn uptime_seconds() -> u64 {
    APP_START
        .get()
        .map(|start| start.elapsed().as_secs())
        .unwrap_or(0)
}

// ─── Log file path ────────────────────────────────────────────────────────────

/// Return the path to the application log directory.
/// Uses the platform app data directory so logs survive application updates.
pub fn log_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_log_dir()
        .unwrap_or_else(|_| PathBuf::from("logs"))
}

// ─── Log sanitizer ────────────────────────────────────────────────────────────

/// Redact sensitive patterns from a log line before including it in a support bundle.
///
/// Patterns redacted:
/// - `password=...`  `pwd=...`  `passwd=...`  (any case, up to next space/quote)
/// - `token=...`  `bearer ...`  `authorization: ...`  (HTTP header forms)
/// - 64-character hex strings (raw key material)
/// - Base64 strings longer than 64 characters (potential secret handles)
/// - `secret_handle=...`  `secret=...`
pub fn sanitize_log_line(line: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static PASSWORD_RE: OnceLock<Regex> = OnceLock::new();
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    static HEX64_RE: OnceLock<Regex> = OnceLock::new();
    static BASE64_LONG_RE: OnceLock<Regex> = OnceLock::new();
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();

    let password_re = PASSWORD_RE.get_or_init(|| {
        Regex::new(r"(?i)(password|pwd|passwd)\s*[=:]\s*\S+").unwrap()
    });
    let token_re = TOKEN_RE.get_or_init(|| {
        // `authorization\s*:\s*.+` captures the full header value (type + credentials).
        // `\S+` alone would stop at the space between "Bearer" and the token value.
        Regex::new(r"(?i)(bearer\s+\S+|token\s*[=:]\s*\S+|authorization\s*:\s*.+)").unwrap()
    });
    let hex64_re = HEX64_RE.get_or_init(|| {
        Regex::new(r"\b[0-9a-fA-F]{64}\b").unwrap()
    });
    let base64_long_re = BASE64_LONG_RE.get_or_init(|| {
        // Base64 alphabet: A-Z a-z 0-9 + / = (padding)
        Regex::new(r"[A-Za-z0-9+/=]{65,}").unwrap()
    });
    let secret_re = SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)(secret_handle|secret_key|api_key|secret)\s*[=:]\s*\S+").unwrap()
    });

    let s = password_re.replace_all(line, "[REDACTED_PASSWORD]").into_owned();
    let s = token_re.replace_all(&s, "[REDACTED_TOKEN]").into_owned();
    let s = hex64_re.replace_all(&s, "[REDACTED_HEX64]").into_owned();
    let s = base64_long_re.replace_all(&s, "[REDACTED_B64]").into_owned();
    let s = secret_re.replace_all(&s, "[REDACTED_SECRET]").into_owned();
    s
}

// ─── AppInfo collection ───────────────────────────────────────────────────────

/// Collect rich application information for the diagnostics bundle.
///
/// Non-fatal: DB query failures produce sensible defaults so the bundle is
/// still usable even if a partial DB failure occurred.
pub async fn collect_diagnostics_app_info(
    app_handle: &tauri::AppHandle,
    db: &DatabaseConnection,
) -> DiagnosticsAppInfo {
    use sysinfo::System;

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
    let arch = std::env::consts::ARCH.to_string();

    // DB schema version = number of applied SeaORM migrations
    let db_schema_version: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM seaql_migrations",
            [],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<i64>("", "cnt").ok())
        .unwrap_or(0);

    // Active locale — read from tenant settings; defaults to "fr-CA" if absent
    let active_locale: String = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json FROM app_settings \
             WHERE setting_key = ? AND setting_scope = ? LIMIT 1",
            [
                "locale.primary_language".into(),
                "tenant".into(),
            ],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<String>("", "setting_value_json").ok())
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_else(|| "fr-CA".to_string());

    DiagnosticsAppInfo {
        app_version: app_handle.package_info().version.to_string(),
        os_name,
        os_version,
        arch,
        db_schema_version,
        active_locale,
        sync_status: "not_configured".to_string(), // Phase 2 VPS sync will update this
        uptime_seconds: uptime_seconds(),
    }
}

// ─── Support bundle generation ────────────────────────────────────────────────

/// Read the last `max_lines` lines from today's rolling log file and sanitize them.
/// Returns `(sanitized_lines, warnings)`.
fn read_sanitized_log_lines(log_dir: &Path, max_lines: usize) -> (Vec<String>, Vec<String>) {
    use std::fs;
    use std::io::{BufRead, BufReader};

    let mut lines = Vec::new();
    let mut warnings = Vec::new();

    // tracing-appender daily rotation produces files named `prefix.YYYY-MM-DD`
    // e.g. `maintafox.log.2026-04-05` (the prefix is the second arg to rolling::daily)
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let log_path = log_dir.join(format!("maintafox.log.{today}"));

    match fs::File::open(&log_path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            let raw_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
            // Take the last `max_lines` lines
            let start = raw_lines.len().saturating_sub(max_lines);
            for line in &raw_lines[start..] {
                lines.push(sanitize_log_line(line));
            }
        }
        Err(e) => {
            warnings.push(format!(
                "could not read log file {:?}: {}",
                log_path, e
            ));
        }
    }

    (lines, warnings)
}

/// Generate a complete support bundle (read-only, no network calls, no state mutation).
pub async fn generate_support_bundle(
    app_handle: &tauri::AppHandle,
    db: &DatabaseConnection,
) -> SupportBundle {
    let generated_at = chrono::Utc::now().to_rfc3339();
    let app_info = collect_diagnostics_app_info(app_handle, db).await;
    let log_dir_path = log_dir(app_handle);
    let (log_lines, collection_warnings) = read_sanitized_log_lines(&log_dir_path, 500);

    tracing::info!(
        log_lines_count = log_lines.len(),
        "support bundle generated"
    );

    SupportBundle {
        generated_at,
        app_info,
        log_lines,
        collection_warnings,
    }
}

// ─── Logging initialization ───────────────────────────────────────────────────

/// Configure the tracing subscriber for both console and file output.
///
/// Called once from `main.rs` (or `lib.rs` setup) before the Tauri builder runs.
/// Returns a `WorkerGuard` that must be held for the lifetime of the process —
/// dropping it flushes and closes the log file.
pub fn init_file_logging(log_dir_path: PathBuf) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_appender::non_blocking;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // Daily rotation — files named `maintafox.log.YYYY-MM-DD`
    let file_appender = rolling::daily(log_dir_path, "maintafox.log");
    let (non_blocking_writer, guard) = non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_writer)
        .with_ansi(false)
        .json(); // machine-readable for future log ingestion

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,maintafox=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    guard
}
```

────────────────────────────────────────────────────────────────────
STEP 3 — PATCH src-tauri/src/lib.rs or main.rs
────────────────────────────────────────────────────────────────────
Ensure the diagnostics module is declared:
```rust
pub mod diagnostics;
```

Update startup to record start time and initialize file logging:
```rust
fn main() {
    // Record application start time before anything else
    diagnostics::record_start_time();

    // Initialize logging before the Tauri builder so early startup messages
    // are captured. The guard must be held until the process exits.
    // The actual log dir is available after the AppHandle is created, so we
    // use a platform-standard temp path for the bootstrap guard, then
    // re-initialize inside setup() with the real app log dir.
    // In Phase 1: a single initialization inside the setup closure is sufficient.
    tauri::Builder::default()
        .setup(|app| {
            let log_dir = app.handle()
                .path()
                .app_log_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("logs"));
            std::fs::create_dir_all(&log_dir)?;
            let _guard = diagnostics::init_file_logging(log_dir);
            // NOTE: In production the _guard must be stored somewhere that lives
            // for the process lifetime. Store it in the AppState or in a static.
            // For Phase 1 scaffolding, a process-lifetime Box::leak is acceptable:
            Box::leak(Box::new(_guard));
            Ok(())
        })
        // ... rest of builder ...
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- On app startup, a log file appears in the platform app-log directory
  (e.g., `%LOCALAPPDATA%\systems.maintafox.desktop\logs\maintafox.log.YYYY-MM-DD` on Windows)
- The log file contains structured JSON lines on startup
- `sanitize_log_line("token=abc123secret")` returns `"[REDACTED_TOKEN]"`
```

---

### Supervisor Verification — Sprint S1

**V1 — Log file is created on startup.**
Run `pnpm run tauri dev`. Then navigate to:
- Windows: `%LOCALAPPDATA%\systems.maintafox.desktop\logs\` (Tauri app log dir)
- macOS: `~/Library/Logs/systems.maintafox.desktop/`

Confirm a file named `maintafox.log.YYYY-MM-DD` (today's date) exists. Open it and
verify it contains JSON-formatted log lines. If the file does not exist, the
`init_file_logging()` call in `setup()` did not run or the directory creation failed.

**V2 — Log file contains startup messages.**
Open the log file and confirm at least one of these messages appears:
`migration`, `seed_default_settings`, or `app started`. These confirm the tracing
subscriber is active and writing to the file.

**V3 — Sanitizer rejects secret patterns.**
In `src-tauri/src/diagnostics/mod.rs`, identify the `sanitize_log_line` function.
Verify (by code review or unit test) that the string `"password=hunter2"` is sanitized
to `"[REDACTED_PASSWORD]=hunter2"` or `"[REDACTED_PASSWORD]"`. The exact replacement
text depends on the regex pattern, but the word "hunter2" must not appear in the output.

---

## Sprint S2 — Diagnostics IPC Commands and Sanitizer Tests

### AI Agent Prompt

```
You are a senior Rust and TypeScript engineer. The diagnostics module exists. Write the
IPC commands that expose app-info and bundle generation to the frontend, and write unit
tests for the log sanitizer.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/commands/diagnostics.rs
────────────────────────────────────────────────────────────────────
```rust
//! Diagnostics IPC commands.
//!
//! Both commands require an active session — only authenticated users can
//! request support bundles or application info. This limits exposure in case
//! of IPC injection attempts.

use tauri::State;
use crate::state::AppState;
use crate::errors::AppResult;

/// Return rich application info for the diagnostics panel.
///
/// Richer than the pre-auth `get_app_info` from `commands::app`: includes
/// DB schema version, active locale from `app_settings`, and process uptime.
/// Requires an active authenticated session.
#[tauri::command]
pub async fn get_diagnostics_info(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::diagnostics::DiagnosticsAppInfo> {
    let _user = crate::require_session!(state);
    Ok(crate::diagnostics::collect_diagnostics_app_info(&app, &state.db).await)
}

/// Generate and return a sanitized support bundle.
///
/// Captures the last 500 log lines (sanitized), application metadata, and
/// any non-fatal collection warnings. Read-only: no state changes, no network calls.
/// Requires an active authenticated session (limits accidental IPC exposure).
///
/// NOTE: Bundle generation reads the rolling log file from disk. On slow hardware
/// with a full 500-line day this may take ~100 ms. The frontend should show a
/// loading indicator while waiting.
#[tauri::command]
pub async fn generate_support_bundle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<crate::diagnostics::SupportBundle> {
    let _user = crate::require_session!(state);
    Ok(crate::diagnostics::generate_support_bundle(&app, &state.db).await)
}
```

────────────────────────────────────────────────────────────────────
PATCH src-tauri/src/commands/mod.rs and lib.rs
────────────────────────────────────────────────────────────────────
Add `pub mod diagnostics;` to `commands/mod.rs`.

Add to the `invoke_handler` list in `lib.rs`:
```rust
commands::diagnostics::get_diagnostics_info,
commands::diagnostics::generate_support_bundle,
```

────────────────────────────────────────────────────────────────────
INLINE TESTS — append to src-tauri/src/diagnostics/mod.rs
────────────────────────────────────────────────────────────────────
Add a `#[cfg(test)]` module at the bottom of `src-tauri/src/diagnostics/mod.rs`
(no separate file — tests live alongside the sanitizer logic):

```rust
#[cfg(test)]
mod tests {
    use super::sanitize_log_line;

    #[test]
    fn redacts_password_key_value() {
        let input = "Failed login: username=admin password=hunter2 retry=true";
        let output = sanitize_log_line(input);
        assert!(!output.contains("hunter2"), "password was not redacted: {}", output);
        assert!(output.contains("[REDACTED_PASSWORD]"), "expected REDACTED_PASSWORD marker: {}", output);
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Outgoing request: Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig";
        let output = sanitize_log_line(input);
        assert!(!output.contains("eyJhbGciOiJSUzI1NiJ9"), "token was not redacted: {}", output);
    }

    #[test]
    fn redacts_64_char_hex() {
        let input = "key_material=a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2";
        let output = sanitize_log_line(input);
        assert!(!output.contains("a3f1b2c4d5e6f7a8"), "hex was not redacted: {}", output);
    }

    #[test]
    fn redacts_long_base64() {
        let input = "secret_handle=AAABBBCCCDDDEEEFFFGGGHHHIIIJJJKKKLLLMMMNNNOOOPPPQQQRRRSSSTTTUUUVVVWWW=";
        let output = sanitize_log_line(input);
        assert!(!output.contains("AAABBBCCC"), "base64 was not redacted: {}", output);
    }

    #[test]
    fn preserves_normal_log_lines() {
        let input = "2026-04-01T12:00:00Z INFO maintafox: migration 007 applied";
        let output = sanitize_log_line(input);
        assert_eq!(input, output, "normal line should not be modified");
    }

    #[test]
    fn redacts_token_equals_form() {
        let input = "config: token=sk-live-abc123secret456";
        let output = sanitize_log_line(input);
        assert!(!output.contains("sk-live-abc123secret456"), "token form not redacted: {}", output);
    }
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo test` passes: 6 sanitizer tests pass, 0 failures
- `cargo check` passes with 0 errors
- DevTools: `await window.__TAURI__.invoke('get_diagnostics_info')` (logged-in session)
  returns an object with `app_version`, `os_name`, `arch`, `db_schema_version`
- DevTools: `await window.__TAURI__.invoke('generate_support_bundle')` returns an object
  with `log_lines` as an array (may be empty if the log file was just created)
- `generate_support_bundle` called without a session returns an auth error
```

---

### Supervisor Verification — Sprint S2

**V1 — Sanitizer unit tests pass.**
Run `cargo test --package maintafox-desktop diagnostics::tests` (or `cargo test` from
the `src-tauri/` directory). Confirm 6 tests pass, 0 failures. If any sanitizer test
fails, review the regex patterns in `sanitize_log_line` — specifically check that the
regex is not anchored too tightly (it should match mid-string occurrences).

**V2 — get_diagnostics_info returns valid shape.**
Log in as the admin user. Open DevTools Console and run:
```javascript
const info = await window.__TAURI__.invoke('get_diagnostics_info');
console.log(info);
```
The result object must contain `app_version` (non-empty string), `os_name` (non-empty),
`db_schema_version` (number ≥ 7, since migration 007 is applied), and `uptime_seconds`
(number > 0).

**V3 — Support bundle contains sanitized log lines.**
Run `generate_support_bundle` from DevTools. Inspect the `log_lines` array. If the
array is non-empty, confirm none of the strings contain patterns matching `password=`,
`Bearer `, or 64-character hex strings. The sanitizer must have processed all lines.

---

## Sprint S3 — Frontend Diagnostics Service and Support Bundle Dialog

### AI Agent Prompt

```
You are a TypeScript and React engineer. The Rust diagnostics commands are registered.
Write the frontend service and the support bundle dialog stub.

────────────────────────────────────────────────────────────────────
PATCH shared/ipc-types.ts — add diagnostics types
────────────────────────────────────────────────────────────────────
```typescript
// ─── Diagnostics / Support Bundle (SP06-F03) ──────────────────────────────────────

/** Rich application metadata from `get_diagnostics_info` (session-gated). */
export interface DiagnosticsAppInfo {
  app_version: string;
  os_name: string;
  os_version: string;
  arch: string;
  db_schema_version: number;
  active_locale: string;
  sync_status: string;
  uptime_seconds: number;
}

/** Sanitized support bundle returned by `generate_support_bundle`. */
export interface SupportBundle {
  generated_at: string;
  app_info: DiagnosticsAppInfo;
  log_lines: string[];
  collection_warnings: string[];
}
```

────────────────────────────────────────────────────────────────────
PATCH src/services/diagnostics-service.ts
────────────────────────────────────────────────────────────────────
The file already exists from integrity-check work. Add the SP06-F03 schemas
and service functions alongside the existing integrity functions.

```typescript
// ADR-003 compliant: all IPC calls for diagnostics go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for diagnostics operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { DiagnosticsAppInfo, IntegrityReport, SupportBundle } from "@shared/ipc-types";

// ── Zod schemas for runtime shape validation ──────────────────────────────────

const IntegrityIssueSchema = z.object({
  code: z.string(),
  description: z.string(),
  is_auto_repairable: z.boolean(),
  subject: z.string(),
});

export const IntegrityReportSchema = z.object({
  is_healthy: z.boolean(),
  is_recoverable: z.boolean(),
  issues: z.array(IntegrityIssueSchema),
  seed_schema_version: z.number().int().nullable(),
  domain_count: z.number().int(),
  value_count: z.number().int(),
});

// ── SP06-F03 Zod schemas ─────────────────────────────────────────────

const DiagnosticsAppInfoSchema = z.object({
  app_version: z.string(),
  os_name: z.string(),
  os_version: z.string(),
  arch: z.string(),
  db_schema_version: z.number(),
  active_locale: z.string(),
  sync_status: z.string(),
  uptime_seconds: z.number(),
});

const SupportBundleSchema = z.object({
  generated_at: z.string(),
  app_info: DiagnosticsAppInfoSchema,
  log_lines: z.array(z.string()),
  collection_warnings: z.array(z.string()),
});

// ── Service functions — Integrity ───────────────────────────────────────

export async function runIntegrityCheck(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("run_integrity_check");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}

export async function repairSeedData(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("repair_seed_data");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}

// ── Service functions — SP06-F03 Diagnostics & Support Bundle ─────────────

/**
 * Return rich application metadata (session-gated).
 * Richer than the pre-auth `get_app_info` — includes DB schema version,
 * locale from settings, and process uptime.
 */
export async function getDiagnosticsInfo(): Promise<DiagnosticsAppInfo> {
  const raw = await invoke<unknown>("get_diagnostics_info");
  return DiagnosticsAppInfoSchema.parse(raw) as DiagnosticsAppInfo;
}

/**
 * Generate and return a sanitized support bundle (session-gated).
 * Contains the last 500 log lines (sanitized), app info, and any
 * non-fatal collection warnings.
 */
export async function generateSupportBundle(): Promise<SupportBundle> {
  const raw = await invoke<unknown>("generate_support_bundle");
  return SupportBundleSchema.parse(raw) as SupportBundle;
}
```

────────────────────────────────────────────────────────────────────
CREATE src/components/SupportBundleDialog.tsx
────────────────────────────────────────────────────────────────────
```tsx
/**
 * SupportBundleDialog.tsx
 *
 * Phase 1 stub. Shows application info and allows copying a diagnostic bundle
 * to the clipboard. Full ticketing integration is a Phase 2 feature.
 *
 * Used from: Settings → Support & Diagnostics (Phase 2 UI module)
 * For Phase 1 it is accessible from the developer debug menu only.
 */

import { ClipboardCopy, Download, Loader2, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { getDiagnosticsInfo, generateSupportBundle } from "@/services/diagnostics-service";
import type { DiagnosticsAppInfo, SupportBundle } from "@shared/ipc-types";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SupportBundleDialog({ open, onClose }: Props) {
  const { t } = useTranslation("diagnostics");
  const [appInfo, setAppInfo] = useState<DiagnosticsAppInfo | null>(null);
  const [bundle, setBundle] = useState<SupportBundle | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleGenerate = useCallback(async () => {
    setIsGenerating(true);
    setError(null);
    try {
      const info = await getDiagnosticsInfo();
      const b = await generateSupportBundle();
      setAppInfo(info);
      setBundle(b);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsGenerating(false);
    }
  }, []);

  const handleCopy = useCallback(async () => {
    if (!bundle) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(bundle, null, 2));
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 3000);
    } catch {
      setError(t("copyFailed"));
    }
  }, [bundle, t]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("dialogTitle")}
        className="w-full max-w-lg rounded-xl bg-surface-1 border border-surface-border shadow-xl"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-surface-border px-5 py-4">
          <h2 className="text-lg font-semibold text-text-primary">{t("dialogTitle")}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("close")}
            className="rounded-lg p-1 text-text-muted hover:bg-surface-2 hover:text-text-primary"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Body */}
        <div className="space-y-4 px-5 py-4">
          <p className="text-sm text-text-secondary">{t("description")}</p>

          {appInfo && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt className="text-text-muted">{t("version")}</dt>
              <dd className="font-mono text-text-primary">{appInfo.app_version}</dd>
              <dt className="text-text-muted">{t("os")}</dt>
              <dd className="text-text-primary">
                {appInfo.os_name} {appInfo.os_version} ({appInfo.arch})
              </dd>
              <dt className="text-text-muted">{t("dbSchema")}</dt>
              <dd className="text-text-primary">
                {t("migrationCount", { count: appInfo.db_schema_version })}
              </dd>
              <dt className="text-text-muted">{t("locale")}</dt>
              <dd className="text-text-primary">{appInfo.active_locale}</dd>
              <dt className="text-text-muted">{t("uptime")}</dt>
              <dd className="text-text-primary">
                {Math.floor(appInfo.uptime_seconds / 60)} {t("minutes")}
              </dd>
            </dl>
          )}

          {bundle && bundle.collection_warnings.length > 0 && (
            <ul role="list" className="space-y-1 rounded-lg bg-status-warning/10 border border-status-warning/30 p-3">
              {bundle.collection_warnings.map((w, i) => (
                <li key={i} className="text-xs text-status-warning">{w}</li>
              ))}
            </ul>
          )}

          {error && (
            <p role="alert" className="rounded-lg bg-status-error/10 border border-status-error/30 p-3 text-sm text-status-error">
              {error}
            </p>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-surface-border px-5 py-4">
          <button
            type="button"
            onClick={() => void handleGenerate()}
            disabled={isGenerating}
            className={cn(
              "inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium",
              "bg-primary text-white hover:bg-primary/90 disabled:opacity-50",
            )}
          >
            {isGenerating ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            {isGenerating ? t("generating") : t("generateBundle")}
          </button>

          <button
            type="button"
            onClick={() => void handleCopy()}
            disabled={!bundle || isGenerating}
            className={cn(
              "inline-flex items-center gap-2 rounded-lg border border-surface-border px-4 py-2 text-sm font-medium",
              "bg-surface-2 text-text-primary hover:bg-surface-3 disabled:opacity-50",
            )}
          >
            <ClipboardCopy className="h-4 w-4" />
            {copySuccess ? t("copied") : t("copyToClipboard")}
          </button>

          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-surface-border bg-surface-2 px-4 py-2 text-sm font-medium text-text-primary hover:bg-surface-3"
          >
            {t("close")}
          </button>
        </div>
      </div>
    </div>
  );
}
```

────────────────────────────────────────────────────────────────────
ADD translation keys for diagnostics namespace
────────────────────────────────────────────────────────────────────
Create `src/i18n/locale-data/en/diagnostics.json`:
```json
{
  "dialogTitle": "Support & Diagnostics",
  "description": "Generate a diagnostic report to share with the support team.",
  "version": "Version",
  "os": "Operating System",
  "dbSchema": "Database Schema",
  "migrationCount": "{{count}} migrations applied",
  "locale": "Active Locale",
  "uptime": "Uptime",
  "minutes": "minutes",
  "generateBundle": "Generate Support Bundle",
  "generating": "Generating…",
  "copyToClipboard": "Copy to Clipboard",
  "copied": "Copied!",
  "copyFailed": "Failed to copy to clipboard.",
  "close": "Close"
}
```

Create `src/i18n/locale-data/fr/diagnostics.json`:
```json
{
  "dialogTitle": "Support et diagnostics",
  "description": "Générer un rapport de diagnostic à partager avec l'équipe de support.",
  "version": "Version",
  "os": "Système d'exploitation",
  "dbSchema": "Schéma de base de données",
  "migrationCount": "{{count}} migrations appliquées",
  "locale": "Langue active",
  "uptime": "Temps de fonctionnement",
  "minutes": "minutes",
  "generateBundle": "Générer le rapport de diagnostic",
  "generating": "Génération en cours…",
  "copyToClipboard": "Copier dans le presse-papier",
  "copied": "Copié !",
  "copyFailed": "Échec de la copie dans le presse-papier.",
  "close": "Fermer"
}
```

────────────────────────────────────────────────────────────────────
REGISTER the diagnostics i18n namespace
────────────────────────────────────────────────────────────────────
Three files must be patched so the namespace is lazy-loaded, type-safe,
and covered by the parity checker:

1. **`src/i18n/namespaces.ts`** — add `diagnostics: "diagnostics"` to
   the `MODULE_NAMESPACES` object so the namespace is lazy-loaded on demand.

2. **`src/i18n/types.ts`** — add `diagnostics` to the
   `CustomTypeOptions.resources` interface so `useTranslation("diagnostics")`
   is accepted by the TypeScript compiler:
   ```typescript
   import type frDiagnostics from "./locale-data/fr/diagnostics.json";
   // ... inside CustomTypeOptions.resources:
   diagnostics: typeof frDiagnostics;
   ```

3. **`scripts/check-i18n-parity.ts`** — add `"diagnostics"` to the
   hardcoded `MODULE_NAMESPACES` array so `pnpm run i18n:check` covers it.

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- `pnpm run i18n:check` passes — no missing translation keys for the diagnostics namespace
- `SupportBundleDialog` renders without errors when opened from the debug menu
- Clicking "Generate Support Bundle" populates the app info section
- Clicking "Copy to Clipboard" writes valid JSON to the clipboard
- The component is accessible: role=dialog, aria-modal, aria-label are present
```

---

### Supervisor Verification — Sprint S3

**V1 — Type check passes.**
Run `pnpm run typecheck`. Zero errors means `DiagnosticsAppInfo`, `SupportBundle`, and the component
prop types are all consistent.

**V2 — Translation parity.**
Run `pnpm run i18n:check`. Both `en/diagnostics.json` and `fr/diagnostics.json` must
contain all keys used in `SupportBundleDialog.tsx`. If keys are missing, `i18n:check`
should report them.

**V3 — Dialog renders (visual smoke test).**
In development, open the application and navigate to the developer debug menu or trigger
the dialog from the browser console. Click "Generate Support Bundle". The application
info section (`version`, `os`, `dbSchema`) should populate within 2 seconds. If the
section stays empty, check for console errors — likely the `get_diagnostics_info` command was
not called or returned an error.

**V4 — Clipboard behavior.**
With the dialog open and a bundle generated, click "Copy to Clipboard". Paste the content
into a text editor. The pasted content should be valid JSON matching the `SupportBundle`
shape: `{ "generated_at": "...", "app_info": { ... }, "log_lines": [...], ... }`.
If the pasted content is empty, `navigator.clipboard.writeText` is not available in the
Tauri WebView — check that the CSP headers do not block clipboard access.

---

## Implementation Notes (Post-Execution Amendments)

> This section documents deviations from the original spec that were applied during
> implementation due to real codebase constraints discovered at build time.
> The code blocks and deliverable table above have been updated to reflect the
> actual implementation. This section serves as an audit trail.

### Naming Collisions Resolved

| Original Spec Name | Actual Name | Reason |
|---|---|---|
| Rust struct `AppInfo` | `DiagnosticsAppInfo` | Avoids collision with existing `AppInfoResponse` (pre-auth) in `commands::app` and `ipc-types.ts` |
| IPC command `get_app_info` | `get_diagnostics_info` | `commands::app::get_app_info` already registered in the invoke handler; Tauri rejects duplicate command names |
| Rust function `collect_app_info` | `collect_diagnostics_app_info` | Renamed to match the struct it returns |
| TS function `getAppInfo()` | `getDiagnosticsInfo()` | Mirrors the IPC command rename |

### Database Layer Adaptation (sqlx → SeaORM)

The original spec used `sqlx::query_scalar(...)` and `sqlx::SqlitePool`. The project
uses **SeaORM 1.x** throughout. All raw SQL was rewritten to use
`ConnectionTrait::query_one()` with `Statement::from_sql_and_values(DbBackend::Sqlite, ...)`.
The `pool` parameter was replaced with `db: &DatabaseConnection`.

### State Architecture Adaptation

The spec assumed separate `State<'_, AuthState>` and `State<'_, sqlx::SqlitePool>` Tauri
managed states. The project uses a unified `AppState` struct that holds `db: DbPool`,
`session: Arc<RwLock<SessionManager>>`, `config: Arc<RwLock<AppConfig>>`, and
`tasks: BackgroundTaskSupervisor`. Commands take `State<'_, AppState>` only, and the
session check is `crate::require_session!(state)` (macro in `crate::auth`).

### Authorization Regex Fix

The token regex pattern `authorization\s*:\s*\S+` was changed to
`authorization\s*:\s*.+` because `\S+` stops at whitespace — it would capture
`Authorization:` but not the `Bearer <token>` that follows the space. The `.+` variant
captures the entire header value.

### Log Filename Pattern

`tracing-appender::rolling::daily("dir", "maintafox.log")` produces files named
`maintafox.log.YYYY-MM-DD`, **not** `maintafox.YYYY-MM-DD.log` as originally assumed.
The `read_sanitized_log_lines` function and all path references were updated accordingly.

### Frontend Conventions

| Spec Assumed | Actual Convention | Reason |
|---|---|---|
| `useT("diagnostics")` custom hook | `useTranslation("diagnostics")` from `react-i18next` | Project does not have a custom `useT` hook |
| `public/locales/{en,fr}/diagnostics.json` | `src/i18n/locale-data/{en,fr}/diagnostics.json` | Project convention; namespaces are lazy-loaded from `src/i18n/locale-data/` |
| Bare unstyled HTML elements | Tailwind + design tokens (`surface-1`, `text-primary`, etc.) + lucide-react icons | Aligned to existing component styling patterns |
| `import from "../services/..."` | `import from "@/services/..."` | Project uses path aliases (`@/` → `src/`) |

### Additional i18n Registration (Not in Original Spec)

Three files required patching beyond the locale JSON files:

1. **`src/i18n/namespaces.ts`** — `diagnostics` added to `MODULE_NAMESPACES` for lazy-loading.
2. **`src/i18n/types.ts`** — `diagnostics` added to `CustomTypeOptions.resources` for type-safe `t()` calls.
3. **`scripts/check-i18n-parity.ts`** — `diagnostics` added to its hardcoded namespace list.

### Test Location

The spec deliverable table listed `src/__tests__/diagnostics/sanitizer.test.ts` (a
TypeScript test file). The tests were instead written as a Rust inline `#[cfg(test)]`
module at the bottom of `diagnostics/mod.rs`. This is more direct — the sanitizer is a
Rust function, so Rust-native tests avoid the IPC mocking overhead that a TS test would
require. All 6 test scenarios from the spec are present and passing.

### Cargo.toml Additions Beyond Original Table

- `regex = "1"` — mentioned in Step 3 of the original spec but missing from the
  deliverable table; now included in Step 1.
- `"json"` feature added to the existing `tracing-subscriber` dependency — required for
  the `.json()` call on the file logging layer.

---

*End of Phase 1 · Sub-phase 06 · File 03*

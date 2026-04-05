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

// ─── Application start time ───────────────────────────────────────────────────
// Stored as a process-lifetime OnceLock so uptime is available anywhere without
// holding a handle.

static APP_START: OnceLock<Instant> = OnceLock::new();

/// Record the application start instant. Call once before the Tauri builder runs.
pub fn record_start_time() {
    APP_START.get_or_init(Instant::now);
}

/// Return seconds elapsed since `record_start_time()` was called.
/// Returns 0 if `record_start_time()` was never called (test or early-crash path).
pub fn uptime_seconds() -> u64 {
    APP_START.get().map(|start| start.elapsed().as_secs()).unwrap_or(0)
}

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
    /// Number of applied SeaORM migrations (from `seaql_migrations`)
    pub db_schema_version: i64,
    /// Active locale code, e.g. "fr-CA"
    pub active_locale: String,
    /// Sync status label — "not_configured" in Phase 1; Phase 2 VPS sync will update this
    pub sync_status: String,
    /// Seconds since the application process started
    pub uptime_seconds: u64,
}

/// Support bundle — all fields are safe to share with the support team.
/// Log lines are sanitized before inclusion (no secrets, tokens, or key material).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportBundle {
    /// ISO 8601 timestamp when the bundle was generated
    pub generated_at: String,
    /// Application metadata snapshot
    pub app_info: DiagnosticsAppInfo,
    /// Up to 500 sanitized log lines, oldest first
    pub log_lines: Vec<String>,
    /// Non-fatal warnings encountered while collecting bundle data (e.g. missing log file)
    pub collection_warnings: Vec<String>,
}

// ─── Log file path ────────────────────────────────────────────────────────────

/// Return the platform app-log directory path.
/// Falls back to a local `logs/` directory if the Tauri path resolver fails.
pub fn log_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_log_dir()
        .unwrap_or_else(|_| PathBuf::from("logs"))
}

// ─── Log sanitizer ────────────────────────────────────────────────────────────

/// Redact sensitive patterns from a single log line before including it in a bundle.
///
/// Patterns redacted (case-insensitive where noted):
/// - `password=…`, `pwd=…`, `passwd=…`
/// - `bearer …`, `token=…`, `authorization: …`
/// - 64-character hex strings (raw key material)
/// - Base64 strings longer than 64 characters (potential secret handles / JWTs)
/// - `secret_handle=…`, `secret_key=…`, `api_key=…`, `secret=…`
///
/// This is a second line of defense. Primary defense is disciplined log hygiene
/// (log user IDs not names/credentials; never log secret values).
pub fn sanitize_log_line(line: &str) -> String {
    use regex::Regex;

    static PASSWORD_RE: OnceLock<Regex> = OnceLock::new();
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    static HEX64_RE: OnceLock<Regex> = OnceLock::new();
    static BASE64_LONG_RE: OnceLock<Regex> = OnceLock::new();
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();

    let password_re = PASSWORD_RE.get_or_init(|| Regex::new(r"(?i)(password|pwd|passwd)\s*[=:]\s*\S+").unwrap());
    let token_re = TOKEN_RE.get_or_init(|| {
        // `authorization\s*:\s*.+` captures the full header value (type + credentials).
        // `\S+` alone would stop at the space between "Bearer" and the token value.
        Regex::new(r"(?i)(bearer\s+\S+|token\s*[=:]\s*\S+|authorization\s*:\s*.+)").unwrap()
    });
    let hex64_re = HEX64_RE.get_or_init(|| Regex::new(r"\b[0-9a-fA-F]{64}\b").unwrap());
    // Base64 alphabet: A-Z a-z 0-9 + / = (padding)
    let base64_long_re = BASE64_LONG_RE.get_or_init(|| Regex::new(r"[A-Za-z0-9+/=]{65,}").unwrap());
    let secret_re =
        SECRET_RE.get_or_init(|| Regex::new(r"(?i)(secret_handle|secret_key|api_key|secret)\s*[=:]\s*\S+").unwrap());

    let s = password_re.replace_all(line, "[REDACTED_PASSWORD]").into_owned();
    let s = token_re.replace_all(&s, "[REDACTED_TOKEN]").into_owned();
    let s = hex64_re.replace_all(&s, "[REDACTED_HEX64]").into_owned();
    let s = base64_long_re.replace_all(&s, "[REDACTED_B64]").into_owned();
    secret_re.replace_all(&s, "[REDACTED_SECRET]").into_owned()
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
            ["locale.primary_language".into(), "tenant".into()],
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
        sync_status: "not_configured".to_string(),
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
            // Keep the tail — oldest-first slice of the last `max_lines` lines
            let start = raw_lines.len().saturating_sub(max_lines);
            for line in &raw_lines[start..] {
                lines.push(sanitize_log_line(line));
            }
        }
        Err(e) => {
            warnings.push(format!("could not read log file {log_path:?}: {e}"));
        }
    }

    (lines, warnings)
}

/// Generate a complete support bundle (read-only, no network calls, no state mutation).
pub async fn generate_support_bundle(app_handle: &tauri::AppHandle, db: &DatabaseConnection) -> SupportBundle {
    let generated_at = chrono::Utc::now().to_rfc3339();
    let app_info = collect_diagnostics_app_info(app_handle, db).await;
    let log_dir_path = log_dir(app_handle);
    let (log_lines, collection_warnings) = read_sanitized_log_lines(&log_dir_path, 500);

    tracing::info!(
        log_lines_count = log_lines.len(),
        "diagnostics support bundle generated"
    );

    SupportBundle {
        generated_at,
        app_info,
        log_lines,
        collection_warnings,
    }
}

// ─── Logging initialization ───────────────────────────────────────────────────

/// Configure the tracing subscriber for dual console + rolling-file output.
///
/// Replaces the simple console-only subscriber initialized in earlier sub-phases.
/// Must be called **once** from `lib.rs` setup(), after the AppHandle is available
/// so the platform log directory can be resolved.
///
/// Returns a `WorkerGuard` that MUST be kept alive for the process lifetime —
/// dropping it flushes the async writer and closes the log file.
/// In Phase 1 it is acceptable to `Box::leak` the guard in the setup closure.
pub fn init_file_logging(log_dir_path: PathBuf) -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_appender::non_blocking;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // Daily rotation — files named `maintafox.YYYY-MM-DD.log`
    let file_appender = rolling::daily(log_dir_path, "maintafox.log");
    let (non_blocking_writer, guard) = non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_writer)
        .with_ansi(false)
        .json(); // machine-readable JSON for future log ingestion

    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,maintafox=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    guard
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::sanitize_log_line;

    #[test]
    fn redacts_password_key_value() {
        let input = "Failed login: username=admin password=hunter2 retry=true";
        let output = sanitize_log_line(input);
        assert!(!output.contains("hunter2"), "password was not redacted: {output}");
        assert!(
            output.contains("[REDACTED_PASSWORD]"),
            "expected REDACTED_PASSWORD marker: {output}"
        );
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Outgoing request: Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig";
        let output = sanitize_log_line(input);
        assert!(
            !output.contains("eyJhbGciOiJSUzI1NiJ9"),
            "token was not redacted: {output}"
        );
    }

    #[test]
    fn redacts_64_char_hex() {
        let input = "key_material=a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2";
        let output = sanitize_log_line(input);
        assert!(!output.contains("a3f1b2c4d5e6f7a8"), "hex was not redacted: {output}");
    }

    #[test]
    fn redacts_long_base64() {
        let input = "secret_handle=AAABBBCCCDDDEEEFFFGGGHHHIIIJJJKKKLLLMMMNNNOOOPPPQQQRRRSSSTTTUUUVVVWWW=";
        let output = sanitize_log_line(input);
        assert!(!output.contains("AAABBBCCC"), "base64 was not redacted: {output}");
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
        assert!(
            !output.contains("sk-live-abc123secret456"),
            "token form not redacted: {output}"
        );
    }
}

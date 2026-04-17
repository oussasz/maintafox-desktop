# Phase 1 · Sub-phase 06 · File 01
# Settings Core and Policy Loading

## Context and Purpose

Sub-phases 01–05 delivered the full engineering baseline, Tauri shell, local data plane,
authentication system with trusted-device and RBAC plumbing, and the multilingual
foundation. The application now has a working runtime, migrations, sessions, permissions,
and bilingual translation infrastructure.

Sub-phase 06 closes Phase 1. Its job is to complete the **operational control plane** —
the scaffolding that the VPS later extends and Phase 2 modules depend on. File 01
specifically delivers the settings persistence layer: the database tables that store
tenant settings, policy snapshots, and audit events for sensitive setting changes, plus
the Rust service functions and IPC commands that all Phase 2 modules will call when they
need to read or write governed configuration.

The settings system in Maintafox is not a simple key/value store. The PRD (§6.18) defines
a **Draft → Test → Activate → Revert** governance workflow for high-risk settings
(connector credentials, session policy, backup policy) and a direct-apply path for
low-risk presentation settings. This file builds the data layer for that governance
workflow — the actual UI surface is a Phase 2 deliverable.

## Architecture Rules Applied

- **Two-tier risk model.** A `setting_risk` field on every setting row classifies
  whether the change is `low` (apply immediately, audit only) or `high` (must go through
  Draft → Test → Activate workflow and requires `require_step_up!` at activation).
- **Permission gate:** all settings writes require `adm.settings`. The `require_permission!`
  macro from SP04-F03 is used at the IPC command layer.
- **Step-up gate:** all high-risk settings activations require `require_step_up!` in
  addition to `adm.settings`. Step-up verification was implemented in SP04-F03.
- **Audit trail on every write.** Every settings change inserts a row in
  `settings_change_events` with a SHA-256 hash of the old and new values. Setting
  values themselves never leave the Rust layer as plaintext when they contain secret
  handles — only the `secret_ref_id` is stored in `app_settings`; the actual secret
  lives in the OS keychain.
- **Policy loading at startup.** Session policy (idle timeout, offline grace) is loaded
  from `policy_snapshots` during startup and injected into the session manager. If no
  active policy snapshot is found, the hardcoded safe defaults from SP04-F01 are used.
  This prevents a missing settings migration from making the session system non-functional.
- **Migration number 007.** This migration follows migration 006 (teams and skills) from
  SP03-F03. The naming convention established in SP03-F02 is followed:
  `m20260401_000007_settings_tables.rs`.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000007_settings_tables.rs` | Five settings tables |
| `src-tauri/src/settings/mod.rs` | Settings service: load, get, set, hash, policy loading |
| `src-tauri/src/commands/settings.rs` | IPC: `get_setting`, `set_setting`, `get_policy_snapshot`, `list_setting_change_events` |
| `src-tauri/src/lib.rs` (patch) | Register settings IPC commands in Tauri builder |
| `shared/ipc-types.ts` (patch) | `AppSetting`, `PolicySnapshot`, `SettingsChangeEvent` types |
| `src/services/settings-service.ts` | Frontend IPC wrappers with Zod validation |
| `src/stores/settings-store.ts` | Zustand store for settings UI state (read cache only) |

## Prerequisites

- SP03-F02 complete: migration naming convention established, Migrator in place
- SP04-F03 complete: `require_permission!` and `require_step_up!` macros available
- SP04-F01 complete: `app_sessions`, `trusted_devices`, `user_accounts` schema present

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Settings Tables Migration | `m20260401_000007_settings_tables.rs`, Migrator update |
| S2 | Settings Service and IPC Commands | `settings/mod.rs`, `commands/settings.rs`, lib.rs patch |
| S3 | Frontend Service, Store, and Startup Policy Load | `settings-service.ts`, `settings-store.ts`, startup policy injection |

---

## Sprint S1 — Settings Tables Migration

### AI Agent Prompt

```
You are a senior Rust and SQLite engineer. Migrations 001–006 are complete as established
in SP01–SP04. Your task is to write migration 007 which creates the settings persistence
tables for the Maintafox Settings & Configuration Center (PRD §6.18).

────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/migrations/m20260401_000007_settings_tables.rs
────────────────────────────────────────────────────────────────────

This migration creates FIVE tables:
  1. app_settings — key/value store for all tenant, device, and user-default settings
  2. secure_secret_refs — OS-keychain reference records (no plaintext secrets)
  3. connection_profiles — integration connector state (SMTP, ERP, IoT, DMS, Power BI)
  4. policy_snapshots — versioned activated policy documents (session, backup, etc.)
  5. settings_change_events — immutable audit log of every setting write

```rust
// src-tauri/migrations/m20260401_000007_settings_tables.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000007_settings_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ─── 1. app_settings ─────────────────────────────────────────────────
        // Stores all governed configuration keys for the tenant deployment.
        // setting_scope: "tenant" | "device" | "user_default"
        // setting_risk:  "low" (apply immediately) | "high" (Draft→Test→Activate)
        // validation_status: "valid" | "draft" | "error" | "untested"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("app_settings"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("setting_key"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("setting_scope"))
                            .string()
                            .not_null()
                            .default("tenant"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("setting_value_json"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("category"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("setting_risk"))
                            .string()
                            .not_null()
                            .default("low"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("validation_status"))
                            .string()
                            .not_null()
                            .default("valid"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("secret_ref_id"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_modified_by_id"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_modified_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .index(
                        Index::create()
                            .if_not_exists()
                            .name("uq_app_settings_key_scope")
                            .table(Alias::new("app_settings"))
                            .col(Alias::new("setting_key"))
                            .col(Alias::new("setting_scope"))
                            .unique(),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 2. secure_secret_refs ───────────────────────────────────────────
        // OS keychain reference records. Actual secret bytes live in the OS
        // keychain (Windows Credential Manager / macOS Keychain / libsecret).
        // This table stores the lookup handle and rotation metadata only.
        // secret_scope: "smtp" | "sms" | "erp" | "iot" | "dms" | "power_bi"
        // backend_type: "windows_credential_manager" | "mac_keychain" | "libsecret"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("secure_secret_refs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("secret_scope"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("backend_type"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("secret_handle"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("label"))
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_rotated_at"))
                            .timestamp()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_validated_at"))
                            .timestamp()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 3. connection_profiles ───────────────────────────────────────────
        // Connector configuration profiles (SMTP, ERP, IoT, DMS, Power BI).
        // status: "draft" | "tested" | "active" | "error" | "retired"
        // Secrets are referenced via secret_ref_id, never stored inline.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("connection_profiles"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("integration_type"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("profile_name"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("config_json"))
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("secret_ref_id"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .string()
                            .not_null()
                            .default("draft"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_tested_at"))
                            .timestamp()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("last_test_result"))
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 4. policy_snapshots ──────────────────────────────────────────────
        // Versioned, activated policy documents.
        // policy_domain: "session" | "notification" | "document_access" |
        //                "sync" | "backup" | "recovery"
        // Only the most recently activated snapshot per domain is "current".
        // Superseded snapshots are retained for audit.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("policy_snapshots"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("policy_domain"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("version_no"))
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Alias::new("snapshot_json"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Alias::new("activated_at"))
                            .timestamp()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("activated_by_id"))
                            .integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 5. settings_change_events ────────────────────────────────────────
        // Immutable audit log — rows are never updated or deleted.
        // old_value_hash and new_value_hash are SHA-256 hex strings so values
        // are never stored in plaintext in the audit log.
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("settings_change_events"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("setting_key_or_domain"))
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("change_summary"))
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(
                        ColumnDef::new(Alias::new("old_value_hash"))
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("new_value_hash"))
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("changed_by_id"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("changed_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Alias::new("required_step_up"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Alias::new("apply_result"))
                            .string()
                            .not_null()
                            .default("applied"),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // NOTE: down() is for local developer resets ONLY.
        // Never run against production data (see docs/MIGRATION_GUIDE.md).
        manager.drop_table(Table::drop().table(Alias::new("settings_change_events")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("policy_snapshots")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("connection_profiles")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("secure_secret_refs")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("app_settings")).to_owned()).await?;
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — Register Migration 007 in the Migrator
────────────────────────────────────────────────────────────────────
Open `src-tauri/migrations/lib.rs` (or `src-tauri/src/db/migrator.rs` depending on the
structure from SP03). Add `m20260401_000007_settings_tables` to the migrations vec:

```rust
// In the Migrator::migrations() impl:
vec![
    Box::new(m20260331_000001_system_tables::Migration),
    Box::new(m20260331_000002_user_tables::Migration),
    Box::new(m20260331_000003_reference_domains::Migration),
    Box::new(m20260331_000004_org_structure::Migration),
    Box::new(m20260331_000005_equipment_registry::Migration),
    Box::new(m20260331_000006_teams_and_skills::Migration),
    // SP06 — Settings tables
    Box::new(m20260401_000007_settings_tables::Migration),
]
```

────────────────────────────────────────────────────────────────────
STEP 3 — Insert default settings rows in the seeder
────────────────────────────────────────────────────────────────────
Open `src-tauri/src/db/seeder.rs` (established in SP03-F04). Add a seeder function
`seed_default_settings` that inserts the following **default settings** only if the
`app_settings` table is empty. This function is called from the startup sequence after
migrations are applied:

```rust
/// Default app_settings rows. These establish the safe-defaults baseline for a
/// fresh installation. All defaults are low-risk and apply immediately.
const DEFAULT_SETTINGS: &[(&str, &str, &str, &str)] = &[
    // (setting_key, category, setting_scope, setting_value_json)
    ("locale.primary_language",      "localization",  "tenant",  r#""fr""#),
    ("locale.fallback_language",     "localization",  "tenant",  r#""en""#),
    ("locale.date_format",           "localization",  "tenant",  r#""DD/MM/YYYY""#),
    ("locale.number_format",         "localization",  "tenant",  r#""fr-FR""#),
    ("locale.week_start_day",        "localization",  "tenant",  r#"1"#),
    ("appearance.color_mode",        "appearance",    "tenant",  r#""light""#),
    ("appearance.density",           "appearance",    "tenant",  r#""standard""#),
    ("appearance.text_scale",        "appearance",    "tenant",  r#"1.0"#),
    ("updater.release_channel",      "system",        "device",  r#""stable""#),
    ("updater.auto_check",           "system",        "device",  r#"true"#),
    ("backup.retention_daily",       "backup",        "tenant",  r#"7"#),
    ("backup.retention_weekly",      "backup",        "tenant",  r#"4"#),
    ("backup.retention_monthly",     "backup",        "tenant",  r#"12"#),
    ("diagnostics.log_retention_days","system",       "device",  r#"30"#),
];

pub async fn seed_default_settings(db: &DatabaseConnection) -> Result<(), DbErr> {
    use sea_orm::EntityTrait;
    // Check if any settings already exist
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_settings")
        .fetch_one(db.get_sqlite_connection_pool())
        .await
        .unwrap_or(0);

    if count > 0 {
        tracing::debug!("seed_default_settings: settings already present, skipping");
        return Ok(());
    }

    let now = chrono::Utc::now().naive_utc();
    for (key, category, scope, value) in DEFAULT_SETTINGS {
        sqlx::query(
            "INSERT OR IGNORE INTO app_settings
             (setting_key, category, setting_scope, setting_value_json, setting_risk,
              validation_status, last_modified_at)
             VALUES (?, ?, ?, ?, 'low', 'valid', ?)"
        )
        .bind(key)
        .bind(category)
        .bind(scope)
        .bind(value)
        .bind(now)
        .execute(db.get_sqlite_connection_pool())
        .await?;
    }

    tracing::info!("seed_default_settings: {} default settings inserted", DEFAULT_SETTINGS.len());
    Ok(())
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` inside src-tauri passes with 0 errors
- `cargo test` passes: migration 007 applies and schema_versions shows 7 migrations
- DBeaver (or SQLite browser) shows the five new tables after startup
- `app_settings` has 14 rows after first launch
- `settings_change_events` table is empty (no writes yet — reads never audit)
```

---

### Supervisor Verification — Sprint S1

**V1 — Migration applies without error.**
Start the application with `pnpm run tauri dev`. In the terminal log output, look for a
line containing `migration 007` or `m20260401_000007_settings_tables` and the word
`applied`. If startup fails with a migration error, copy the full error message and flag it.

**V2 — Settings tables exist.**
Open DB Browser for SQLite (or equivalent tool) and open the application database file.
Navigate to the Tables list. Confirm you see all five new tables:
`app_settings`, `secure_secret_refs`, `connection_profiles`, `policy_snapshots`,
`settings_change_events`. If any are missing, the migration did not apply.

**V3 — Default settings were seeded.**
In the same DB tool, open `app_settings` and run `SELECT COUNT(*) FROM app_settings;`.
The result must be 14 (the number of default settings rows). If the count is 0, the seeder
did not run. Check the startup log for "seed_default_settings" messages.

**V4 — Audit table is append-only (no DELETE/UPDATE permissions).**
This is a design verification, not a runtime check. In the code review, confirm that
`settings_change_events` has no UPDATE or DELETE operations anywhere in the codebase.
The table definition has no `ON DELETE CASCADE` or `ON UPDATE` foreign key constraints.
Only INSERT operations are permitted on this table.

---

## Sprint S2 — Settings Service and IPC Commands

### AI Agent Prompt

```
You are a senior Rust engineer. Migration 007 is applied and the five settings tables
exist. Your task is to write the settings service (business logic) and the IPC command
functions that the frontend calls.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/settings/mod.rs
────────────────────────────────────────────────────────────────────
```rust
//! Settings service.
//!
//! Provides typed access to the `app_settings`, `policy_snapshots`, and
//! `settings_change_events` tables. All writes through this module emit an audit
//! event. Secret values are never stored in `setting_value_json` — callers store
//! only the `secret_ref_id` foreign key.

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::fmt;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSetting {
    pub id: i64,
    pub setting_key: String,
    pub setting_scope: String,
    pub setting_value_json: String,
    pub category: String,
    pub setting_risk: String,
    pub validation_status: String,
    pub secret_ref_id: Option<i64>,
    pub last_modified_by_id: Option<i64>,
    pub last_modified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySnapshot {
    pub id: i64,
    pub policy_domain: String,
    pub version_no: i64,
    pub snapshot_json: String,
    pub is_active: bool,
    pub activated_at: Option<String>,
    pub activated_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsChangeEvent {
    pub id: i64,
    pub setting_key_or_domain: String,
    pub change_summary: String,
    pub old_value_hash: Option<String>,
    pub new_value_hash: Option<String>,
    pub changed_by_id: Option<i64>,
    pub changed_at: String,
    pub required_step_up: bool,
    pub apply_result: String,
}

/// Session policy loaded from the `policy_snapshots` table.
/// Used by the session manager on startup and after policy activation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPolicy {
    pub idle_timeout_minutes: u32,
    pub absolute_session_minutes: u32,
    pub offline_grace_hours: u32,
    pub step_up_window_minutes: u32,
    pub max_failed_attempts: u32,
    pub lockout_minutes: u32,
}

impl Default for SessionPolicy {
    /// Safe defaults — used when no policy snapshot exists in the database.
    /// These match the hardcoded defaults from SP04-F01 exactly so a fresh
    /// installation has consistent behaviour before an admin activates a policy.
    fn default() -> Self {
        Self {
            idle_timeout_minutes: 30,
            absolute_session_minutes: 480,
            offline_grace_hours: 72,
            step_up_window_minutes: 15,
            max_failed_attempts: 5,
            lockout_minutes: 15,
        }
    }
}

// ─── Hash helper ──────────────────────────────────────────────────────────────

/// Produce a SHA-256 hex digest of a value string.
/// Used for audit-trail hashing — never for security-critical purposes.
pub fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

// ─── Read functions ───────────────────────────────────────────────────────────

/// Return a single setting by key and scope.
/// Returns `None` if the key does not exist.
pub async fn get_setting(
    pool: &SqlitePool,
    key: &str,
    scope: &str,
) -> AppResult<Option<AppSetting>> {
    let row = sqlx::query_as!(
        AppSetting,
        r#"SELECT id, setting_key, setting_scope, setting_value_json, category,
                  setting_risk, validation_status, secret_ref_id,
                  last_modified_by_id,
                  strftime('%Y-%m-%dT%H:%M:%SZ', last_modified_at) AS "last_modified_at!: String"
           FROM app_settings
           WHERE setting_key = ? AND setting_scope = ?"#,
        key,
        scope
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Return all settings in a given category.
pub async fn list_settings_by_category(
    pool: &SqlitePool,
    category: &str,
) -> AppResult<Vec<AppSetting>> {
    let rows = sqlx::query_as!(
        AppSetting,
        r#"SELECT id, setting_key, setting_scope, setting_value_json, category,
                  setting_risk, validation_status, secret_ref_id,
                  last_modified_by_id,
                  strftime('%Y-%m-%dT%H:%M:%SZ', last_modified_at) AS "last_modified_at!: String"
           FROM app_settings
           WHERE category = ?
           ORDER BY setting_key"#,
        category
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Return the currently active `PolicySnapshot` for a given domain.
/// Returns `None` if no snapshot has been activated yet.
pub async fn get_active_policy(
    pool: &SqlitePool,
    domain: &str,
) -> AppResult<Option<PolicySnapshot>> {
    let row = sqlx::query_as!(
        PolicySnapshot,
        r#"SELECT id, policy_domain, version_no, snapshot_json, is_active,
                  strftime('%Y-%m-%dT%H:%M:%SZ', activated_at) AS "activated_at: String",
                  activated_by_id
           FROM policy_snapshots
           WHERE policy_domain = ? AND is_active = TRUE
           ORDER BY version_no DESC
           LIMIT 1"#,
        domain
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Load the session policy for use at startup. Returns `SessionPolicy::default()`
/// if no active snapshot exists. Never returns an error — a missing policy falls
/// back to safe defaults rather than preventing startup.
pub async fn load_session_policy(pool: &SqlitePool) -> SessionPolicy {
    match get_active_policy(pool, "session").await {
        Ok(Some(snap)) => {
            serde_json::from_str::<SessionPolicy>(&snap.snapshot_json)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "session policy snapshot is malformed, using defaults: {}",
                        e
                    );
                    SessionPolicy::default()
                })
        }
        Ok(None) => {
            tracing::debug!("no active session policy snapshot found, using defaults");
            SessionPolicy::default()
        }
        Err(e) => {
            tracing::error!("failed to load session policy from DB: {}, using defaults", e);
            SessionPolicy::default()
        }
    }
}

// ─── Write functions ──────────────────────────────────────────────────────────

/// Write a setting value. Always audits the change.
/// `changed_by_id`: the user_accounts.id of the actor.
/// `change_summary`: a human-readable description of the change (shown in audit log).
pub async fn set_setting(
    pool: &SqlitePool,
    key: &str,
    scope: &str,
    value_json: &str,
    changed_by_id: i64,
    change_summary: &str,
) -> AppResult<()> {
    // Validation: reject empty or clearly invalid JSON
    if value_json.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            format!("setting_value_json for '{}' must not be empty", key),
        ]));
    }
    let _: serde_json::Value = serde_json::from_str(value_json).map_err(|_| {
        AppError::ValidationFailed(vec![format!(
            "setting_value_json for '{}' is not valid JSON",
            key
        )])
    })?;

    // Read existing value for audit hash
    let old_hash = get_setting(pool, key, scope)
        .await?
        .map(|s| sha256_hex(&s.setting_value_json));

    let new_hash = sha256_hex(value_json);
    let now = chrono::Utc::now().naive_utc();

    // Upsert the setting
    sqlx::query!(
        r#"INSERT INTO app_settings
           (setting_key, setting_scope, setting_value_json, category,
            setting_risk, validation_status, last_modified_by_id, last_modified_at)
           VALUES (?, ?, ?, 'general', 'low', 'valid', ?, ?)
           ON CONFLICT(setting_key, setting_scope)
           DO UPDATE SET
               setting_value_json = excluded.setting_value_json,
               last_modified_by_id = excluded.last_modified_by_id,
               last_modified_at = excluded.last_modified_at"#,
        key,
        scope,
        value_json,
        changed_by_id,
        now
    )
    .execute(pool)
    .await?;

    // Write audit event — this row is never updated or deleted
    sqlx::query!(
        r#"INSERT INTO settings_change_events
           (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
            changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, ?, ?, ?, ?, FALSE, 'applied')"#,
        key,
        change_summary,
        old_hash,
        new_hash,
        changed_by_id,
        now
    )
    .execute(pool)
    .await?;

    tracing::info!(
        setting_key = key,
        scope = scope,
        actor = changed_by_id,
        "setting updated"
    );
    Ok(())
}

/// Return the N most recent change events (default 50).
pub async fn list_change_events(
    pool: &SqlitePool,
    limit: i64,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let rows = sqlx::query_as!(
        SettingsChangeEvent,
        r#"SELECT id, setting_key_or_domain, change_summary,
                  old_value_hash, new_value_hash, changed_by_id,
                  strftime('%Y-%m-%dT%H:%M:%SZ', changed_at) AS "changed_at!: String",
                  required_step_up, apply_result
           FROM settings_change_events
           ORDER BY changed_at DESC
           LIMIT ?"#,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/commands/settings.rs
────────────────────────────────────────────────────────────────────
```rust
//! IPC commands for the Settings module.
//!
//! All write commands require the `adm.settings` permission.
//! High-risk setting activations additionally require step-up authentication
//! via `require_step_up!`.

use crate::{
    auth::AuthState,
    errors::{AppError, AppResult},
    settings::{
        self, AppSetting, PolicySnapshot, SettingsChangeEvent,
        SessionPolicy,
    },
};
use serde::{Deserialize, Serialize};
use tauri::State;

// ─── GetSetting ──────────────────────────────────────────────────────────────

/// Read a single setting by key. No auth required — settings are readable by
/// any authenticated user. (Secrets are never in setting_value_json.)
#[tauri::command]
pub async fn get_setting(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    key: String,
    scope: Option<String>,
) -> AppResult<Option<AppSetting>> {
    let _user = require_session!(state);
    let scope = scope.unwrap_or_else(|| "tenant".to_string());
    settings::get_setting(&pool, &key, &scope).await
}

// ─── SetSetting ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetSettingPayload {
    pub key: String,
    pub scope: Option<String>,
    pub value_json: String,
    pub change_summary: Option<String>,
}

/// Write a setting value. Requires `adm.settings` permission.
/// For high-risk settings (setting_risk = "high"), also requires recent step-up auth.
#[tauri::command]
pub async fn set_setting(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    payload: SetSettingPayload,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");

    // For high-risk settings, step-up is enforced
    let existing = settings::get_setting(
        &pool,
        &payload.key,
        payload.scope.as_deref().unwrap_or("tenant"),
    )
    .await?;
    if let Some(ref s) = existing {
        if s.setting_risk == "high" {
            require_step_up!(state);
        }
    }

    let scope = payload.scope.unwrap_or_else(|| "tenant".to_string());
    let summary = payload
        .change_summary
        .unwrap_or_else(|| format!("Setting '{}' updated via IPC", payload.key));

    settings::set_setting(
        &pool,
        &payload.key,
        &scope,
        &payload.value_json,
        user.user_id,
        &summary,
    )
    .await
}

// ─── GetPolicySnapshot ───────────────────────────────────────────────────────

/// Return the active policy snapshot for a domain (e.g., "session", "backup").
/// Returns null if no snapshot has been activated yet.
#[tauri::command]
pub async fn get_policy_snapshot(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    domain: String,
) -> AppResult<Option<PolicySnapshot>> {
    let _user = require_session!(state);
    settings::get_active_policy(&pool, &domain).await
}

// ─── GetSessionPolicy ────────────────────────────────────────────────────────

/// Return the resolved session policy (active snapshot OR safe defaults).
/// The login screen calls this before a session exists to load idle-timeout
/// display configuration. No auth required.
#[tauri::command]
pub async fn get_session_policy(
    pool: State<'_, sqlx::SqlitePool>,
) -> AppResult<SessionPolicy> {
    Ok(settings::load_session_policy(&pool).await)
}

// ─── ListSettingChangeEvents ─────────────────────────────────────────────────

/// Return recent settings change events for the audit log UI.
/// Requires `adm.settings` permission.
#[tauri::command]
pub async fn list_setting_change_events(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    limit: Option<i64>,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");
    settings::list_change_events(&pool, limit.unwrap_or(50)).await
}
```

────────────────────────────────────────────────────────────────────
PATCH src-tauri/src/lib.rs — register settings commands
────────────────────────────────────────────────────────────────────
In the `tauri::generate_handler![]` macro call, add the four new settings commands:

```rust
.invoke_handler(tauri::generate_handler![
    // SP01 — health check
    commands::health_check,
    // SP04 — auth commands
    commands::auth::login,
    commands::auth::logout,
    commands::auth::get_session,
    commands::auth::unlock_session,
    commands::auth::force_change_password,
    // SP04-F03 — RBAC commands
    commands::rbac::get_my_permissions,
    commands::rbac::verify_step_up,
    // SP05 — locale commands
    commands::locale::get_locale_preference,
    commands::locale::set_locale_preference,
    // SP06-F01 — settings commands
    commands::settings::get_setting,
    commands::settings::set_setting,
    commands::settings::get_policy_snapshot,
    commands::settings::get_session_policy,
    commands::settings::list_setting_change_events,
])
```

Also ensure `src-tauri/src/commands/mod.rs` includes:
```rust
pub mod settings;
```

And `src-tauri/src/lib.rs` includes `pub mod settings;`.

────────────────────────────────────────────────────────────────────
ADD sha2 to Cargo.toml
────────────────────────────────────────────────────────────────────
The `sha256_hex` function requires the `sha2` crate. Add to `[dependencies]` in
`src-tauri/Cargo.toml`:
```toml
sha2 = "0.10"
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- `cargo test` passes: settings service unit tests pass
- `get_session_policy` IPC returns safe defaults when invoked from DevTools before
  a policy snapshot is activated
- `set_setting` with key "appearance.color_mode" (low-risk) succeeds and inserts
  a row in `settings_change_events`
- `set_setting` with a non-existent permission returns `AppError::Permission`
```

---

### Supervisor Verification — Sprint S2

**V1 — IPC commands respond correctly without admin session.**
Open `pnpm run tauri dev`. In DevTools → Console, run:
```javascript
await window.__TAURI__.invoke('get_session_policy')
```
Expected: an object with keys `idle_timeout_minutes`, `offline_grace_hours`, etc., and
values that match the safe defaults (idle=30, grace=72). If you get an error, the command
was not registered in the invoke handler.

**V2 — Write permission is enforced.**
Without an active admin session, run:
```javascript
await window.__TAURI__.invoke('set_setting', {
  payload: { key: 'appearance.color_mode', value_json: '"dark"' }
})
```
Expected: the promise rejects with an error containing `"not authenticated"` or `"session"`.
If it does not reject, the `require_session!` guard is missing.

**V3 — Audit event is written on successful setting change.**
Log in as the admin user. Then run `set_setting` from DevTools with a valid session token.
Open the database and run `SELECT * FROM settings_change_events;`. Exactly one new row
should appear with the key, a SHA-256 hash in `new_value_hash`, and a timestamp.

---

## Sprint S3 — Frontend Service, Store, and Startup Policy Load

### AI Agent Prompt

```
You are a TypeScript and React engineer. The Rust settings IPC commands are registered.
Your task is to write the frontend service wrapping those commands, a lightweight Zustand
store for settings state, and to update the startup sequence to load the session policy
before the auth screen renders.

────────────────────────────────────────────────────────────────────
PATCH shared/ipc-types.ts — add settings types
────────────────────────────────────────────────────────────────────
```typescript
// Add to existing shared/ipc-types.ts

export interface AppSetting {
  id: number;
  setting_key: string;
  setting_scope: string;
  setting_value_json: string;
  category: string;
  setting_risk: "low" | "high";
  validation_status: "valid" | "draft" | "error" | "untested";
  secret_ref_id: number | null;
  last_modified_by_id: number | null;
  last_modified_at: string; // ISO 8601
}

export interface PolicySnapshot {
  id: number;
  policy_domain: string;
  version_no: number;
  snapshot_json: string; // JSON-encoded policy document
  is_active: boolean;
  activated_at: string | null;
  activated_by_id: number | null;
}

export interface SettingsChangeEvent {
  id: number;
  setting_key_or_domain: string;
  change_summary: string;
  old_value_hash: string | null;
  new_value_hash: string | null;
  changed_by_id: number | null;
  changed_at: string;
  required_step_up: boolean;
  apply_result: string;
}

export interface SessionPolicy {
  idle_timeout_minutes: number;
  absolute_session_minutes: number;
  offline_grace_hours: number;
  step_up_window_minutes: number;
  max_failed_attempts: number;
  lockout_minutes: number;
}
```

────────────────────────────────────────────────────────────────────
CREATE src/services/settings-service.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * settings-service.ts
 *
 * IPC wrappers for the settings commands. All invoke() calls in this service
 * go through Zod validation on the way out to ensure type safety.
 *
 * RULE: This file is the only place in the frontend that calls
 *       invoke('get_setting') / invoke('set_setting') etc.
 *       Components and stores import from this file — never directly from
 *       @tauri-apps/api/core.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  AppSetting,
  PolicySnapshot,
  SessionPolicy,
  SettingsChangeEvent,
} from "@shared/ipc-types";

// ─── Zod schemas ──────────────────────────────────────────────────────────────

const AppSettingSchema = z.object({
  id: z.number(),
  setting_key: z.string(),
  setting_scope: z.string(),
  setting_value_json: z.string(),
  category: z.string(),
  setting_risk: z.enum(["low", "high"]),
  validation_status: z.enum(["valid", "draft", "error", "untested"]),
  secret_ref_id: z.number().nullable(),
  last_modified_by_id: z.number().nullable(),
  last_modified_at: z.string(),
});

const SessionPolicySchema = z.object({
  idle_timeout_minutes: z.number(),
  absolute_session_minutes: z.number(),
  offline_grace_hours: z.number(),
  step_up_window_minutes: z.number(),
  max_failed_attempts: z.number(),
  lockout_minutes: z.number(),
});

const SettingsChangeEventSchema = z.object({
  id: z.number(),
  setting_key_or_domain: z.string(),
  change_summary: z.string(),
  old_value_hash: z.string().nullable(),
  new_value_hash: z.string().nullable(),
  changed_by_id: z.number().nullable(),
  changed_at: z.string(),
  required_step_up: z.boolean(),
  apply_result: z.string(),
});

// ─── Service functions ────────────────────────────────────────────────────────

export async function getSetting(
  key: string,
  scope?: string
): Promise<AppSetting | null> {
  const raw = await invoke<AppSetting | null>("get_setting", { key, scope });
  if (raw === null) return null;
  return AppSettingSchema.parse(raw);
}

export interface SetSettingPayload {
  key: string;
  scope?: string;
  value_json: string;
  change_summary?: string;
}

export async function setSetting(payload: SetSettingPayload): Promise<void> {
  await invoke<void>("set_setting", { payload });
}

export async function getSessionPolicy(): Promise<SessionPolicy> {
  const raw = await invoke<SessionPolicy>("get_session_policy");
  return SessionPolicySchema.parse(raw);
}

export async function getPolicySnapshot(
  domain: string
): Promise<PolicySnapshot | null> {
  return invoke<PolicySnapshot | null>("get_policy_snapshot", { domain });
}

export async function listSettingChangeEvents(
  limit?: number
): Promise<SettingsChangeEvent[]> {
  const raw = await invoke<SettingsChangeEvent[]>("list_setting_change_events", {
    limit,
  });
  return z.array(SettingsChangeEventSchema).parse(raw);
}
```

────────────────────────────────────────────────────────────────────
CREATE src/stores/settings-store.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * settings-store.ts
 *
 * Zustand store for the settings control plane. Acts as a read-through cache:
 * - `sessionPolicy` is loaded once at startup and refreshed on policy activation.
 * - Individual setting values are NOT cached here (too many to enumerate);
 *   they are fetched on demand by the Settings UI module.
 *
 * This store has NO write methods — settings writes go directly through
 * settingsService.setSetting() followed by a targeted refetch.
 */

import { create } from "zustand";

import { getSessionPolicy } from "../services/settings-service";
import type { SessionPolicy } from "@shared/ipc-types";

interface SettingsState {
  /** Session policy — loaded at startup, defines idle/offline behavior */
  sessionPolicy: SessionPolicy | null;
  /** True while the policy is being loaded from the backend */
  policyLoading: boolean;
  /** Error encountered during policy load (should never happen — fallback to defaults) */
  policyError: string | null;

  /** Load the active session policy from the backend. */
  loadSessionPolicy: () => Promise<void>;
  /** Set the session policy directly (used after admin activates a new policy snapshot). */
  applySessionPolicy: (policy: SessionPolicy) => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  sessionPolicy: null,
  policyLoading: false,
  policyError: null,

  loadSessionPolicy: async () => {
    set({ policyLoading: true, policyError: null });
    try {
      const policy = await getSessionPolicy();
      set({ sessionPolicy: policy, policyLoading: false });
    } catch (err) {
      set({
        policyLoading: false,
        policyError: err instanceof Error ? err.message : String(err),
      });
      // Do NOT throw — missing policy uses safe defaults from the Rust side.
    }
  },

  applySessionPolicy: (policy) => set({ sessionPolicy: policy }),
}));
```

────────────────────────────────────────────────────────────────────
PATCH src/App.tsx — load session policy at startup
────────────────────────────────────────────────────────────────────
In `App.tsx` (established in SP02), add a `useEffect` that calls
`useSettingsStore.getState().loadSessionPolicy()` on application mount.
This must run before the auth screen is shown so that idle-timeout display
configuration is available immediately:

```tsx
// In App.tsx, after other store initializations (locale, auth):
import { useEffect } from "react";
import { useSettingsStore } from "./stores/settings-store";

// Inside the App component:
useEffect(() => {
  void useSettingsStore.getState().loadSessionPolicy();
}, []);
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `pnpm run typecheck` passes with 0 errors
- `pnpm run dev`: on startup, DevTools → Application → Zustand shows
  `useSettingsStore.sessionPolicy` is populated after 1–2 seconds
- `getSetting("appearance.color_mode")` returns the default value `"light"` (as JSON)
- `setSetting({ key: "appearance.color_mode", value_json: '"dark"' })` from a test
  script calls the IPC and a new row appears in `settings_change_events`
```

---

### Supervisor Verification — Sprint S3

**V1 — Session policy loads at startup.**
Open `pnpm run tauri dev`. Wait for the login screen. Open DevTools Console and run:
```javascript
// Access Zustand store state (requires Zustand DevTools or direct access)
window.__ZUSTAND_SETTINGS_STORE__?.sessionPolicy
// OR use the Redux DevTools extension if Zustand DevTools are installed
```
The `sessionPolicy` should be an object with numeric values. If it shows `null`, the
`loadSessionPolicy()` call in `App.tsx` did not execute. Check the startup log for
"failed to load session policy" warning messages.

**V2 — TypeScript types match Rust output.**
Run `pnpm run typecheck`. Zero errors means the `AppSetting`, `SessionPolicy`, and
`SettingsChangeEvent` TypeScript types in `shared/ipc-types.ts` are consistent with the
data shapes returned by the Rust commands.

**V3 — Settings parity check.**
Run `pnpm run i18n:check` (established in SP05-F04) to confirm the SP06 work did not
accidentally modify any translation files. Exit code must be 0.

---

## Sprint S4 — Web-Parity Gap Closure (Settings Page & Policy Editor UI)

> **Scope** — This file explicitly states "the actual UI surface is a Phase 2
> deliverable" but no Phase 2 file ever defines it. Sprint S4 closes this gap
> by specifying the SettingsPage shell and policy editor panels that consume
> the backend built in Sprints S1–S3.

### S4‑1 — Settings Page (`SettingsPage.tsx`) — GAP SET‑01

```
LOCATION   src/pages/SettingsPage.tsx
ROUTE      /settings (replaces ModulePlaceholder)
STORE      settings-store.ts (already exists — add activeCategory, editingPolicy)
SERVICE    settings-service.ts (already exists)
GUARD      adm.settings permission

DESCRIPTION
Vertical-tab layout with category groups in left sidebar:

  ┌─────────────────────┬──────────────────────────────────────────┐
  │  Settings            │  General Settings                        │
  │                      │                                          │
  │  ▸ General           │  ┌─────────────────┬───────────────────┐ │
  │    App name          │  │ Setting         │ Value             │ │
  │    Default language  │  ├─────────────────┼───────────────────┤ │
  │    Date format       │  │ App name        │ Maintafox CMMS    │ │
  │  ▸ Security          │  │ Default lang    │ [FR ▼]            │ │
  │    Session policy    │  │ Date format     │ [DD/MM/YYYY ▼]   │ │
  │    Password policy   │  │ Timezone        │ [Auto ▼]         │ │
  │    Device trust      │  └─────────────────┴───────────────────┘ │
  │  ▸ Maintenance       │                                          │
  │    SLA defaults      │  [ Save Changes ] (disabled when clean)  │
  │    WO numbering      │                                          │
  │  ▸ Integration       │                                          │
  │    ERP connector     │                                          │
  │    IoT connector     │                                          │
  │  ▸ Backup            │                                          │
  │    Backup schedule   │                                          │
  │    Retention policy  │                                          │
  └─────────────────────┴──────────────────────────────────────────┘

- Categories loaded from settings domains (dynamic, not hardcoded)
- Direct-apply settings: edited inline, saved immediately with toast
- Governed settings (session policy, backup policy, etc.): use Draft → Test →
  Activate workflow via PolicyEditorPanel (see S4-2)
- Governed settings show current active value + "(draft pending)" badge if draft exists
- All setting changes are audited (audit_events table from Sprint S1)
- Search box at top of left sidebar — filters visible categories/settings

ACCEPTANCE CRITERIA
- route /settings loads with category sidebar and settings table
- direct-apply settings save on change with success toast
- governed settings show PolicyEditorPanel (not inline edit)
- page is permission-gated (adm.settings)
- settings changes appear in audit log
```

### S4‑2 — Policy Editor Panels — GAP SET‑02

```
LOCATION   src/components/settings/PolicyEditorPanel.tsx
STORE      settings-store.ts (patch — add draftPolicy, testResults, activatePolicy)
SERVICE    settings-service.ts (patch — add draft/test/activate IPC wrappers)

DESCRIPTION
Replaces inline editing for governed settings (session policy, password policy,
backup policy, connector credentials). Renders inside the right pane of SettingsPage
when a governed setting category is selected:

  ┌───────────────────────────────────────────────────────────────┐
  │  Session Policy                              Status: Active   │
  │                                                               │
  │  Active Configuration:          Draft (if exists):            │
  │  ┌──────────────────────┐       ┌──────────────────────┐     │
  │  │ Max session: 8h      │       │ Max session: 4h      │     │
  │  │ Idle timeout: 30min  │  →    │ Idle timeout: 15min  │     │
  │  │ Step-up: 120s        │       │ Step-up: 60s         │     │
  │  └──────────────────────┘       └──────────────────────┘     │
  │                                                               │
  │  [ Edit Draft ]  [ Test Draft ]  [ Activate ]  [ Discard ]   │
  │                                                               │
  │  Test Results (if run):                                       │
  │  ✅ Session timeout validation passed                         │
  │  ✅ Idle lock threshold within bounds                         │
  │  ⚠️ Step-up window very short (60s) — consider user impact    │
  │                                                               │
  │  Change History:                                              │
  │  2026-04-07 14:00 — admin — Activated v3                     │
  │  2026-04-07 13:55 — admin — Tested draft v4                  │
  │  2026-04-06 09:30 — admin — Created draft v4                 │
  └───────────────────────────────────────────────────────────────┘

Workflow:
  1. "Edit Draft" — opens form fields for the policy (pre-filled from active or
     existing draft). Saves as draft snapshot.
  2. "Test Draft" — runs backend validation rules. Shows test results panel with
     pass/warn/fail indicators.
  3. "Activate" — requires step-up auth for security policies. Promotes draft to
     active. Old active becomes superseded.
  4. "Discard" — deletes draft (confirm dialog).

Side-by-side diff: active vs draft, highlighting changed values in amber.

Policy-specific form fields vary by policy type:
  - Session: max_session_hours, idle_timeout_minutes, step_up_window_seconds
  - Password: min_length, require_uppercase, require_number, max_age_days,
    history_count
  - Backup: schedule_cron, retention_days, include_photos, compression_level

ACCEPTANCE CRITERIA
- draft → test → activate workflow works end-to-end
- test results show pass/warn/fail per validation rule
- activate requires step-up for security policies
- side-by-side diff highlights changed values
- change history loads from policy_snapshots + audit_events
```

### Supervisor Verification — Sprint S4

**V1 — Settings page navigation.**
Login as admin. Navigate to /settings. Verify category sidebar loads. Click "General" →
direct-apply settings appear. Click "Security" → governed settings show policy editor.

**V2 — Direct-apply setting.**
Change "Default language" to EN. Verify toast confirms save. Refresh page → setting
persists.

**V3 — Policy lifecycle.**
Edit session policy draft (change idle timeout). Test draft → verify test results. Activate
→ step-up required → policy becomes active. Verify old active is superseded in change
history.

**V4 — Permission guard.**
Login as non-admin user. Navigate to /settings → redirected or 403.

---

*End of Phase 1 · Sub-phase 06 · File 01*

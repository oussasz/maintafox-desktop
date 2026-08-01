# Phase 1 · Sub-phase 06 · File 04
# Backup, Restore Preflight, and Operational Validation

## Context and Purpose

This is the final file of Sub-phase 06 and of Phase 1 entirely.

Files 01–03 delivered settings persistence, the updater skeleton, and the diagnostics
foundation. This file delivers the last piece required by the Phase 1 exit criteria
(PRD §15): **backup and restore preflight validation**.

The PRD §6.18.6 (Backup and Recovery) specifies:
- Manual and scheduled backup of the local SQLite database
- Encrypted backup output with SHA-256 checksum
- Restore test mode: verifies a backup file's integrity without replacing the live DB
- Factory reset gated by step-up authentication and typed confirmation

Phase 1 does not require the scheduling runtime (that is Phase 2 background services),
but it does require:
1. The backup tables in the database (migration 008)
2. A working manual backup command that produces a checksum-verified file
3. A working restore test (integrity check on a backup without touching live DB)
4. A formal Phase 1 exit checklist that integrates all sub-phases 01–06

The Phase 1 exit checklist (`docs/PHASE1_EXIT_CRITERIA.md`) is the formal gate document.
It must be verified and signed off before Phase 2 work begins. Sprint S3 of this file
produces that document.

## Architecture Rules Applied

- **Migration 008.** Backup tables follow migration 007 (settings tables from F01).
  The naming convention is `m20260401_000008_backup_tables.rs`.
- **SHA-256 checksum.** Every backup run computes a SHA-256 hex digest of the backup
  file bytes and stores it in the `backup_runs` table. Restore validation compares the
  stored digest to a freshly computed digest before proceeding.
- **Restore test mode never touches the live DB.** The restore test copies the backup
  file to a temporary path, opens it as a read-only SQLite connection, and runs
  `PRAGMA integrity_check`. It does not replace the live database under any condition.
  The live-database replacement is a Phase 2 feature (requires VPS sync drain).
- **Factory reset requires step-up + typed confirmation.** Factory reset is the most
  destructive operation in the application. It requires:
  1. `require_permission!(user, "adm.settings")`
  2. `require_step_up!(state)`
  3. A confirmation string typed by the user (not just a checkbox)
  This file creates the IPC command stub with all three gates but does NOT implement
  the actual data deletion (Phase 2, after sync drain and audit trail archive).
- **All backup/restore commands require `adm.settings`.** Phase 2 may introduce a
  dedicated `adm.backup` permission — for Phase 1, `adm.settings` is the gate.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260401_000008_backup_tables.rs` | `backup_policies` + `backup_runs` tables |
| `src-tauri/src/backup/mod.rs` | Backup service: run backup, checksum, list runs, restore test |
| `src-tauri/src/commands/backup.rs` | IPC: `run_manual_backup`, `list_backup_runs`, `validate_backup_file`, `factory_reset_stub` |
| `src-tauri/src/lib.rs` (patch) | Register backup commands |
| `src/services/backup-service.ts` | Frontend IPC wrappers |
| `docs/PHASE1_EXIT_CRITERIA.md` | **Formal Phase 1 exit checklist** — 50+ verification items |

## Prerequisites

- SP06-F01: `app_settings` seeded, `sha256_hex()` function available in `settings` module
- SP04-F03: `require_permission!`, `require_step_up!`, `adm.settings` seeded
- Migration 007 applied (SP06-F01-S1)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Migration 008 and Backup Service | `m20260401_000008_backup_tables.rs`, `backup/mod.rs` |
| S2 | Backup IPC Commands and Frontend Service | `commands/backup.rs`, `backup-service.ts` |
| S3 | Phase 1 Exit Criteria Checklist | `docs/PHASE1_EXIT_CRITERIA.md` |

---

## Sprint S1 — Migration 008 and Backup Service

### AI Agent Prompt

```
You are a senior Rust and SQLite engineer. Migration 007 is applied. Write migration 008
(backup tables) and the backup service module.

────────────────────────────────────────────────────────────────────
STEP 1 — CREATE src-tauri/migrations/m20260401_000008_backup_tables.rs
────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260401_000008_backup_tables.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260401_000008_backup_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ─── 1. backup_policies ───────────────────────────────────────────────
        // Stores the tenant's configured backup policy.
        // encryption_mode: "plaintext" | "aes256"
        // In Phase 1 only "plaintext" is implemented (AES-256 encryption is Phase 2).
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("backup_policies"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("policy_name"))
                            .string()
                            .not_null()
                            .default("default"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("target_directory"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("encryption_mode"))
                            .string()
                            .not_null()
                            .default("plaintext"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_daily"))
                            .integer()
                            .not_null()
                            .default(7),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_weekly"))
                            .integer()
                            .not_null()
                            .default(4),
                    )
                    .col(
                        ColumnDef::new(Alias::new("retention_monthly"))
                            .integer()
                            .not_null()
                            .default(12),
                    )
                    .col(
                        ColumnDef::new(Alias::new("is_active"))
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Alias::new("created_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Alias::new("updated_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ─── 2. backup_runs ───────────────────────────────────────────────────
        // Immutable audit record of every backup execution.
        // status: "success" | "failed" | "partial"
        // trigger: "manual" | "scheduled" | "pre_update"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("backup_runs"))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Alias::new("id"))
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("policy_id"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("trigger"))
                            .string()
                            .not_null()
                            .default("manual"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("status"))
                            .string()
                            .not_null()
                            .default("success"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("output_path"))
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("file_size_bytes"))
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("sha256_checksum"))
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("encryption_mode"))
                            .string()
                            .not_null()
                            .default("plaintext"),
                    )
                    .col(
                        ColumnDef::new(Alias::new("db_schema_version"))
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("started_at"))
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Alias::new("completed_at"))
                            .timestamp()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("error_message"))
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Alias::new("initiated_by_id"))
                            .integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Alias::new("backup_runs")).to_owned()).await?;
        manager.drop_table(Table::drop().table(Alias::new("backup_policies")).to_owned()).await?;
        Ok(())
    }
}
```

────────────────────────────────────────────────────────────────────
STEP 2 — Register Migration 008 in the Migrator
────────────────────────────────────────────────────────────────────
Add to the migrations vec (after migration 007):
```rust
Box::new(m20260401_000008_backup_tables::Migration),
```

────────────────────────────────────────────────────────────────────
STEP 3 — CREATE src-tauri/src/backup/mod.rs
────────────────────────────────────────────────────────────────────
```rust
//! Backup and restore preflight module.
//!
//! Responsibilities:
//! 1. Manual backup: copy the live SQLite DB to a target path, compute SHA-256
//! 2. Backup run logging: insert a row in backup_runs for every execution
//! 3. Restore test mode: integrity check on a backup file without touching live DB
//! 4. Backup run listing: return run history for the Settings UI

use crate::errors::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::Path;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunRecord {
    pub id: i64,
    pub trigger: String,
    pub status: String,
    pub output_path: String,
    pub file_size_bytes: Option<i64>,
    pub sha256_checksum: Option<String>,
    pub encryption_mode: String,
    pub db_schema_version: Option<i64>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub initiated_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunResult {
    pub run_id: i64,
    pub output_path: String,
    pub file_size_bytes: u64,
    pub sha256_checksum: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreTestResult {
    pub backup_path: String,
    pub integrity_ok: bool,
    pub stored_checksum: Option<String>,
    pub computed_checksum: String,
    pub checksum_match: bool,
    pub integrity_check_output: String,
    pub warnings: Vec<String>,
}

// ─── Checksum ─────────────────────────────────────────────────────────────────

/// Compute SHA-256 hex digest of a file's entire contents.
fn compute_file_sha256(path: &Path) -> AppResult<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(AppError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(AppError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ─── Manual Backup ────────────────────────────────────────────────────────────

/// Execute a manual backup of the SQLite database.
///
/// Algorithm:
/// 1. Resolve the live DB file path from the pool connection string
/// 2. Copy the file to `target_path` using SQLite's backup API (WAL-safe)
/// 3. Compute SHA-256 of the output file
/// 4. Insert a `backup_runs` record with status, checksum, and file size
///
/// Phase 1: encryption_mode is always "plaintext". Phase 2 will add AES-256.
pub async fn run_manual_backup(
    pool: &SqlitePool,
    target_path: &str,
    initiated_by_id: i64,
) -> AppResult<BackupRunResult> {
    use std::fs;

    let started_at = chrono::Utc::now().naive_utc();
    tracing::info!(
        target_path = target_path,
        actor = initiated_by_id,
        "manual backup started"
    );

    // Get the DB file path from the pool options
    let db_path = {
        let options = pool.connect_options();
        // sqlx SqliteConnectOptions provides the filename
        options.get_filename().to_string_lossy().into_owned()
    };

    let db_path = Path::new(&db_path);
    let target = Path::new(target_path);

    // Ensure the target directory exists
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(AppError::Io)?;
    }

    // Copy using SQLite WAL-safe online backup via VACUUM INTO
    // This is the recommended approach for hot-copying a SQLite WAL-mode database.
    // It creates a point-in-time consistent copy without locking out other readers.
    let target_path_sql = target_path.replace('\'', "''"); // sanitize for inline SQL
    sqlx::query(&format!("VACUUM INTO '{}'", target_path_sql))
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(e))?;

    // Compute checksum and file size
    let metadata = fs::metadata(target).map_err(AppError::Io)?;
    let file_size = metadata.len();
    let checksum = compute_file_sha256(target)?;

    // Get current schema version
    let db_schema_version: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM seaql_migrations")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

    let completed_at = chrono::Utc::now().naive_utc();

    // Insert backup_runs record
    let run_id: i64 = sqlx::query_scalar!(
        r#"INSERT INTO backup_runs
           (trigger, status, output_path, file_size_bytes, sha256_checksum,
            encryption_mode, db_schema_version, started_at, completed_at,
            initiated_by_id)
           VALUES ('manual', 'success', ?, ?, ?, 'plaintext', ?, ?, ?, ?)
           RETURNING id"#,
        target_path,
        file_size as i64,
        checksum,
        db_schema_version,
        started_at,
        completed_at,
        initiated_by_id
    )
    .fetch_one(pool)
    .await?;

    tracing::info!(
        run_id = run_id,
        checksum = %checksum,
        file_size_bytes = file_size,
        "manual backup completed successfully"
    );

    Ok(BackupRunResult {
        run_id,
        output_path: target_path.to_string(),
        file_size_bytes: file_size,
        sha256_checksum: checksum,
        status: "success".to_string(),
    })
}

// ─── Restore Test ─────────────────────────────────────────────────────────────

/// Validate a backup file without modifying the live database.
///
/// Steps:
/// 1. Confirm the file exists and compute its SHA-256 checksum
/// 2. Look up the stored checksum from backup_runs (if a record exists)
/// 3. Open the backup file as a read-only SQLite connection (to a temp copy)
/// 4. Run PRAGMA integrity_check
/// 5. Return validation results — never replace the live DB
///
/// The caller (IPC command) interprets the result and shows it in the UI.
pub async fn validate_backup_file(
    pool: &SqlitePool,
    backup_path: &str,
) -> AppResult<RestoreTestResult> {
    use std::fs;

    let path = Path::new(backup_path);
    if !path.exists() {
        return Err(AppError::NotFound {
            entity: "backup_file".to_string(),
            id: backup_path.to_string(),
        });
    }

    let computed_checksum = compute_file_sha256(path)?;

    // Look up stored checksum from backup_runs
    let stored_checksum: Option<String> =
        sqlx::query_scalar!(
            "SELECT sha256_checksum FROM backup_runs WHERE output_path = ? ORDER BY id DESC LIMIT 1",
            backup_path
        )
        .fetch_optional(pool)
        .await?
        .flatten();

    let checksum_match = stored_checksum
        .as_deref()
        .map(|s| s == computed_checksum)
        .unwrap_or(false);

    // Copy to a temp path for read-only integrity check
    let temp_dir = std::env::temp_dir();
    let temp_name = format!("maintafox_restore_test_{}.db", chrono::Utc::now().timestamp());
    let temp_path = temp_dir.join(&temp_name);
    fs::copy(path, &temp_path).map_err(AppError::Io)?;

    // Open and run PRAGMA integrity_check on the temp copy
    let integrity_output = {
        let url = format!("sqlite:{}?mode=ro", temp_path.to_string_lossy());
        match sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
        {
            Ok(test_pool) => {
                let result: String = sqlx::query_scalar("PRAGMA integrity_check")
                    .fetch_one(&test_pool)
                    .await
                    .unwrap_or_else(|e| format!("integrity_check failed: {}", e));
                test_pool.close().await;
                result
            }
            Err(e) => format!("could not open backup file: {}", e),
        }
    };

    // Clean up temp file
    let _ = fs::remove_file(&temp_path);

    let integrity_ok = integrity_output.trim() == "ok";

    tracing::info!(
        backup_path = backup_path,
        integrity_ok = integrity_ok,
        checksum_match = checksum_match,
        "restore preflight completed"
    );

    Ok(RestoreTestResult {
        backup_path: backup_path.to_string(),
        integrity_ok,
        stored_checksum,
        computed_checksum,
        checksum_match,
        integrity_check_output: integrity_output,
        warnings: vec![],
    })
}

// ─── List backup runs ─────────────────────────────────────────────────────────

pub async fn list_backup_runs(pool: &SqlitePool, limit: i64) -> AppResult<Vec<BackupRunRecord>> {
    let rows = sqlx::query_as!(
        BackupRunRecord,
        r#"SELECT id, trigger, status, output_path, file_size_bytes, sha256_checksum,
                  encryption_mode, db_schema_version,
                  strftime('%Y-%m-%dT%H:%M:%SZ', started_at) AS "started_at!: String",
                  strftime('%Y-%m-%dT%H:%M:%SZ', completed_at) AS "completed_at: String",
                  error_message, initiated_by_id
           FROM backup_runs
           ORDER BY started_at DESC
           LIMIT ?"#,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- Migration 008 applies: DB Browser shows `backup_policies` and `backup_runs` tables
- `run_manual_backup(pool, "/tmp/test_backup.db", 1)` produces a valid SQLite file
  with a non-empty SHA-256 checksum in `backup_runs`
- `validate_backup_file(pool, "/tmp/test_backup.db")` returns `integrity_ok: true`
  for a good backup and `integrity_ok: false` for a corrupt file
```

---

### Supervisor Verification — Sprint S1

**V1 — Migration 008 applies.**
Run `pnpm run tauri dev`. Check the terminal output for
`m20260401_000008_backup_tables applied`. Open DB Browser and confirm
`backup_policies` and `backup_runs` tables exist. If missing, check that migration
008 was added to the Migrator's Vec.

**V2 — Backup tables have correct columns.**
In DB Browser, inspect the `backup_runs` table schema. Confirm the columns:
`id`, `trigger`, `status`, `output_path`, `file_size_bytes`, `sha256_checksum`,
`encryption_mode`, `started_at`, `completed_at`, `error_message`, `initiated_by_id`.
Any column missing indicates a migration error.

**V3 — SHA-256 checksum is not null after a backup run.**
Run a test from the Rust test suite or from DevTools (after Sprint S2 IPC is available).
After a successful backup run, query:
```sql
SELECT sha256_checksum FROM backup_runs ORDER BY id DESC LIMIT 1;
```
The result must be a 64-character lowercase hex string. If it is NULL, the
`compute_file_sha256` function or the INSERT statement has a bug.

---

## Sprint S2 — Backup IPC Commands and Frontend Service

### AI Agent Prompt

```
You are a Rust and TypeScript engineer. The backup service module exists. Write the
IPC commands and frontend service.

────────────────────────────────────────────────────────────────────
CREATE src-tauri/src/commands/backup.rs
────────────────────────────────────────────────────────────────────
```rust
//! Backup IPC commands.
//!
//! All commands require adm.settings permission.
//! run_manual_backup additionally requires step-up authentication.
//! factory_reset_stub requires step-up + explicit confirmation string.

use crate::{
    auth::AuthState,
    backup::{self, BackupRunRecord, BackupRunResult, RestoreTestResult},
    errors::{AppError, AppResult},
};
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Deserialize)]
pub struct RunManualBackupPayload {
    pub target_path: String,
}

/// Run a manual backup to the specified target path.
/// Requires: active session + adm.settings + recent step-up
#[tauri::command]
pub async fn run_manual_backup(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    payload: RunManualBackupPayload,
) -> AppResult<BackupRunResult> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");
    require_step_up!(state);

    // Basic path sanitization: reject empty or obviously injected paths
    let target = payload.target_path.trim();
    if target.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "target_path must not be empty".to_string(),
        ]));
    }

    backup::run_manual_backup(&pool, target, user.user_id).await
}

/// List recent backup runs for the audit/history UI.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn list_backup_runs(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    limit: Option<i64>,
) -> AppResult<Vec<BackupRunRecord>> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");
    backup::list_backup_runs(&pool, limit.unwrap_or(20)).await
}

/// Validate a backup file's integrity without restoring it.
/// Requires: active session + adm.settings
#[tauri::command]
pub async fn validate_backup_file(
    state: State<'_, AuthState>,
    pool: State<'_, sqlx::SqlitePool>,
    backup_path: String,
) -> AppResult<RestoreTestResult> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");
    backup::validate_backup_file(&pool, &backup_path).await
}

/// Factory reset stub — gates are in place but data deletion is NOT implemented.
///
/// Phase 2 will implement the actual deletion after:
/// 1. VPS sync drain (all unsynced data is uploaded)
/// 2. Audit trail archive (audit events are exported)
/// 3. Secure wipe of all DB tables in dependency order
///
/// In Phase 1 this command validates all security gates and then returns an
/// informational error explaining that factory reset is a Phase 2 feature.
#[derive(Debug, Deserialize)]
pub struct FactoryResetPayload {
    /// The user must type "EFFACER TOUTES LES DONNÉES" (or the English equivalent)
    /// to confirm. This prevents accidental activations.
    pub confirmation_phrase: String,
}

#[tauri::command]
pub async fn factory_reset_stub(
    state: State<'_, AuthState>,
    _pool: State<'_, sqlx::SqlitePool>,
    payload: FactoryResetPayload,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(user, "adm.settings");
    require_step_up!(state);

    // Validate confirmation phrase (accept both FR and EN)
    const FR_PHRASE: &str = "EFFACER TOUTES LES DONNÉES";
    const EN_PHRASE: &str = "ERASE ALL DATA";
    if payload.confirmation_phrase.trim() != FR_PHRASE
        && payload.confirmation_phrase.trim() != EN_PHRASE
    {
        return Err(AppError::ValidationFailed(vec![
            "factory_reset confirmation phrase does not match".to_string(),
        ]));
    }

    tracing::warn!(
        actor = user.user_id,
        "factory_reset_stub: all security gates passed — \
         data deletion deferred to Phase 2 implementation"
    );

    // Return an explicit "not yet implemented" error so the frontend can show
    // a "this feature is coming in a future version" message.
    Err(AppError::Internal(
        "factory_reset: data deletion is implemented in Phase 2 (VPS sync drain required)"
            .to_string(),
    ))
}
```

────────────────────────────────────────────────────────────────────
PATCH src-tauri/src/commands/mod.rs and lib.rs
────────────────────────────────────────────────────────────────────
Add `pub mod backup;` to `commands/mod.rs`.

Add to the invoke_handler in lib.rs:
```rust
commands::backup::run_manual_backup,
commands::backup::list_backup_runs,
commands::backup::validate_backup_file,
commands::backup::factory_reset_stub,
```

Also add `pub mod backup;` to `src-tauri/src/lib.rs`.

────────────────────────────────────────────────────────────────────
PATCH shared/ipc-types.ts — add backup types
────────────────────────────────────────────────────────────────────
```typescript
export interface BackupRunRecord {
  id: number;
  trigger: string;
  status: string;
  output_path: string;
  file_size_bytes: number | null;
  sha256_checksum: string | null;
  encryption_mode: string;
  db_schema_version: number | null;
  started_at: string;
  completed_at: string | null;
  error_message: string | null;
  initiated_by_id: number | null;
}

export interface BackupRunResult {
  run_id: number;
  output_path: string;
  file_size_bytes: number;
  sha256_checksum: string;
  status: string;
}

export interface RestoreTestResult {
  backup_path: string;
  integrity_ok: boolean;
  stored_checksum: string | null;
  computed_checksum: string;
  checksum_match: boolean;
  integrity_check_output: string;
  warnings: string[];
}
```

────────────────────────────────────────────────────────────────────
CREATE src/services/backup-service.ts
────────────────────────────────────────────────────────────────────
```typescript
/**
 * backup-service.ts
 *
 * IPC wrappers for backup and restore preflight commands.
 * RULE: All invoke() calls for backup commands are isolated here.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  BackupRunRecord,
  BackupRunResult,
  RestoreTestResult,
} from "@shared/ipc-types";

export async function runManualBackup(targetPath: string): Promise<BackupRunResult> {
  return invoke<BackupRunResult>("run_manual_backup", {
    payload: { target_path: targetPath },
  });
}

export async function listBackupRuns(limit?: number): Promise<BackupRunRecord[]> {
  return invoke<BackupRunRecord[]>("list_backup_runs", { limit });
}

export async function validateBackupFile(
  backupPath: string
): Promise<RestoreTestResult> {
  return invoke<RestoreTestResult>("validate_backup_file", {
    backup_path: backupPath,
  });
}

export async function factoryResetStub(
  confirmationPhrase: string
): Promise<void> {
  return invoke<void>("factory_reset_stub", {
    payload: { confirmation_phrase: confirmationPhrase },
  });
}
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `cargo check` passes with 0 errors
- `pnpm run typecheck` passes with 0 errors
- DevTools (logged in as admin with step-up): invoke('run_manual_backup', { payload:
  { target_path: '/tmp/backup_test.db' } }) returns a BackupRunResult object with a
  64-char sha256_checksum
- invoke('validate_backup_file', { backup_path: '/tmp/backup_test.db' }) returns
  { integrity_ok: true, checksum_match: true }
- invoke('factory_reset_stub', { payload: { confirmation_phrase: 'WRONG' } }) returns
  a ValidationFailed error
```

---

### Supervisor Verification — Sprint S2

**V1 — Manual backup completes and returns checksum.**
Log in as admin. Perform step-up authentication. In DevTools Console:
```javascript
const result = await window.__TAURI__.invoke('run_manual_backup', {
  payload: { target_path: 'C:\\Temp\\maintafox_test.db' } // adjust path for OS
});
console.log(result);
```
Expected: `{ run_id: 1, output_path: '...', file_size_bytes: > 0, sha256_checksum: '<64 hex chars>', status: 'success' }`.

**V2 — Restore test confirms integrity.**
After V1 succeeds, run:
```javascript
const validation = await window.__TAURI__.invoke('validate_backup_file', {
  backup_path: 'C:\\Temp\\maintafox_test.db'
});
console.log(validation);
```
Expected: `{ integrity_ok: true, checksum_match: true, integrity_check_output: 'ok' }`.

**V3 — Factory reset stub rejects wrong confirmation phrase.**
Run:
```javascript
await window.__TAURI__.invoke('factory_reset_stub', {
  payload: { confirmation_phrase: 'yes please' }
})
```
Expected: promise rejects with a `ValidationFailed` error message mentioning
"confirmation phrase".

---

## Sprint S3 — Phase 1 Exit Criteria Checklist

### AI Agent Prompt

```
You are a senior technical program manager and quality engineer. Phase 1 of Maintafox
consists of 6 sub-phases (SP01–SP06) covering: engineering baseline, Tauri/React/Rust
shell, local data plane, auth/session/RBAC, multilingual foundation, and the operational
control plane (settings, updater, diagnostics, backup). Write the formal exit checklist.

────────────────────────────────────────────────────────────────────
CREATE docs/PHASE1_EXIT_CRITERIA.md
────────────────────────────────────────────────────────────────────
```markdown
# Maintafox — Phase 1 Exit Criteria

> **Status:** DRAFT  
> **Version:** 1.0  
> **Author:** Phase 1 Engineering Team  
> **Purpose:** This document is the formal gate between Phase 1 (Secure Foundation) and
> Phase 2 (Core CMMS Modules). All items in this checklist must pass before Phase 2
> work begins.

---

## How to Use This Document

Each item is marked with a status:
- `[ ]` — not yet verified
- `[x]` — verified and passing
- `[~]` — partial / known limitation accepted
- `[N/A]` — not applicable to this deployment

A Phase 1 release candidate is NOT ready for Phase 2 handoff unless all mandatory items
are either `[x]` or explicitly accepted as `[~]` with a written rationale.

---

## Section 1 — SP01: Engineering Baseline and Toolchain

| # | Item | Status |
|---|------|--------|
| 1.1 | `rustup`, `cargo`, and `tauri-cli` are at the pinned versions in `rust-toolchain.toml` | `[ ]` |
| 1.2 | `pnpm install` completes with no peer dependency warnings | `[ ]` |
| 1.3 | `cargo check --workspace` produces 0 errors | `[ ]` |
| 1.4 | `pnpm run typecheck` produces 0 errors | `[ ]` |
| 1.5 | `pnpm run lint` produces 0 errors (warnings acceptable) | `[ ]` |
| 1.6 | `cargo fmt --check` passes (no un-formatted Rust files) | `[ ]` |
| 1.7 | All 6 bootstrap migrations (001–006) are listed in the Migrator Vec | `[ ]` |
| 1.8 | `AppError` enum covers all required variants: `Database`, `Auth`, `NotFound`, `ValidationFailed`, `Permission`, `Internal`, `Io`, `Serialization` | `[ ]` |

---

## Section 2 — SP02: Tauri/React/Rust Shell

| # | Item | Status |
|---|------|--------|
| 2.1 | `pnpm run tauri dev` starts without panic or compilation error | `[ ]` |
| 2.2 | The application window opens and displays the login screen | `[ ]` |
| 2.3 | `pnpm run tauri build` produces a signed installer (or unsigned in dev if Phase 2 signing task is pending) | `[ ]` |
| 2.4 | The DevTools overlay is disabled in production builds | `[ ]` |
| 2.5 | CSP headers are set to disallow inline scripts and remote script sources | `[ ]` |
| 2.6 | `health_check` IPC command returns `{ status: "ok" }` | `[ ]` |
| 2.7 | The window title and about page display the version from `Cargo.toml` | `[ ]` |

---

## Section 3 — SP03: Local Data Plane

| # | Item | Status |
|---|------|--------|
| 3.1 | Migrations 001–006 apply cleanly on a fresh database (no errors in startup log) | `[ ]` |
| 3.2 | Migrations 001–006 apply idempotently: running startup twice does not create duplicate rows | `[ ]` |
| 3.3 | Schema version count after fresh install equals 8 (migrations 001–008) | `[ ]` |
| 3.4 | Reference domain tables (`equipment_categories`, `asset_statuses`, etc.) are seeded with the baseline French and English labels | `[ ]` |
| 3.5 | `MigrationGuard` prevents application startup if any migration fails | `[ ]` |
| 3.6 | All model files use `sqlx::query_as!` (compile-time checked queries) | `[ ]` |
| 3.7 | Database file is stored in the platform-standard app data directory (not the install directory) | `[ ]` |

---

## Section 4 — SP04: Authentication, Session, and RBAC

| # | Item | Status |
|---|------|--------|
| 4.1 | Admin account created by `insertAdmin.sql` → can log in with correct password | `[ ]` |
| 4.2 | Login with wrong password increments `failed_login_attempts` and returns an error | `[ ]` |
| 4.3 | Account lockout activates after 5 failed attempts and prevents further login | `[ ]` |
| 4.4 | Idle timeout: session expires after 30 minutes of inactivity (or the policy-configured value) | `[ ]` |
| 4.5 | Trusted-device registration creates a row in `trusted_devices` | `[ ]` |
| 4.6 | Trusted-device: a previously registered device skips 2FA on re-login | `[ ]` |
| 4.7 | `require_permission!(user, "perm.code")` returns `AppError::Permission` for a user without the permission | `[ ]` |
| 4.8 | `require_step_up!(state)` returns `AppError::Auth` if step-up was not performed within the window | `[ ]` |
| 4.9 | `adm.settings` permission is seeded and assigned to the Administrator role | `[ ]` |
| 4.10 | `force_change_password` command works: temporary password is rejected on second login | `[ ]` |
| 4.11 | Session token is stored in the Tauri application state, NOT in `localStorage` | `[ ]` |
| 4.12 | All password fields use bcrypt (cost factor ≥ 12) — no plaintext or MD5/SHA1 hashes | `[ ]` |

---

## Section 5 — SP05: Multilingual Foundation

| # | Item | Status |
|---|------|--------|
| 5.1 | Login screen renders in French by default | `[ ]` |
| 5.2 | Language switch in Settings → Localization changes the UI language without restart | `[ ]` |
| 5.3 | `pnpm run i18n:check` passes: 0 missing keys in both `en` and `fr` translation sets | `[ ]` |
| 5.4 | All user-visible strings in SP01–SP05 components go through `useT()` — no hardcoded English strings in component files | `[ ]` |
| 5.5 | Date formatting uses `useFormatters()` — no hardcoded `toLocaleDateString()` calls | `[ ]` |
| 5.6 | Number formatting uses `useFormatters()` — French locale uses comma as decimal separator | `[ ]` |
| 5.7 | Week starts on Monday in the French locale (ISO 8601 compliant) | `[ ]` |
| 5.8 | The locale preference persists across application restarts | `[ ]` |

---

## Section 6 — SP06: Settings, Updater, Diagnostics, and Backup

### 6a — Settings Core

| # | Item | Status |
|---|------|--------|
| 6.1 | Migrations 007 and 008 apply cleanly: all 7 settings/backup tables exist | `[ ]` |
| 6.2 | 14 default setting rows are present in `app_settings` after fresh install | `[ ]` |
| 6.3 | `get_setting` IPC returns the correct default for `appearance.color_mode` | `[ ]` |
| 6.4 | `set_setting` IPC writes a value and inserts a row in `settings_change_events` | `[ ]` |
| 6.5 | `settings_change_events` rows contain SHA-256 hashes, not plaintext values | `[ ]` |
| 6.6 | `set_setting` without a session returns an auth error | `[ ]` |
| 6.7 | `set_setting` for a `setting_risk = "high"` row without step-up returns an auth error | `[ ]` |
| 6.8 | `get_session_policy` returns safe defaults when no policy snapshot exists | `[ ]` |
| 6.9 | `useSettingsStore.sessionPolicy` is populated within 3 seconds of startup | `[ ]` |

### 6b — Updater

| # | Item | Status |
|---|------|--------|
| 6.10 | `tauri-plugin-updater` is listed in `Cargo.toml` dependencies | `[ ]` |
| 6.11 | `check_for_update` IPC returns `{ available: false }` in the Phase 1 stub environment | `[ ]` |
| 6.12 | `install_pending_update` without a session returns an auth error | `[ ]` |
| 6.13 | `updater.release_channel` default setting is `"stable"` | `[ ]` |
| 6.14 | `docs/UPDATER_SIGNING.md` exists and contains the three-key-pair table | `[ ]` |
| 6.15 | `tauri.conf.json` updater pubkey is the Phase 1 placeholder (not a real key yet) | `[ ]` |

### 6c — Diagnostics and Logging

| # | Item | Status |
|---|------|--------|
| 6.16 | A rolling log file is created in the platform app-log directory on startup | `[ ]` |
| 6.17 | Log file contains JSON-structured entries (not plain text) | `[ ]` |
| 6.18 | `get_app_info` IPC returns correct `app_version`, `os_name`, `db_schema_version` | `[ ]` |
| 6.19 | `generate_support_bundle` IPC returns a bundle with sanitized log lines | `[ ]` |
| 6.20 | Sanitizer test: `sanitize_log_line("password=hunter2")` does not contain "hunter2" | `[ ]` |
| 6.21 | All 6 sanitizer unit tests pass (`cargo test diagnostics::tests`) | `[ ]` |
| 6.22 | `SupportBundleDialog` renders without errors and copy-to-clipboard works | `[ ]` |
| 6.23 | `generate_support_bundle` without a session returns an auth error | `[ ]` |

### 6d — Backup and Restore Preflight

| # | Item | Status |
|---|------|--------|
| 6.24 | `run_manual_backup` produces a SQLite file at the specified path | `[ ]` |
| 6.25 | The backup file's SHA-256 checksum is recorded in `backup_runs` | `[ ]` |
| 6.26 | `validate_backup_file` returns `{ integrity_ok: true }` for a valid backup | `[ ]` |
| 6.27 | `validate_backup_file` does NOT modify the live database file | `[ ]` |
| 6.28 | `run_manual_backup` without step-up auth returns an auth error | `[ ]` |
| 6.29 | `factory_reset_stub` with wrong confirmation phrase returns a validation error | `[ ]` |
| 6.30 | `factory_reset_stub` with correct phrase passes all security gates but returns a Phase-2-pending error | `[ ]` |

---

## Section 7 — Security Baseline

| # | Item | Status |
|---|------|--------|
| 7.1 | No hardcoded credentials in any source file or configuration file | `[ ]` |
| 7.2 | No secrets stored in `setting_value_json` — only `secret_ref_id` references | `[ ]` |
| 7.3 | All audit log entries use value hashes, not plaintext values | `[ ]` |
| 7.4 | `settings_change_events` has no UPDATE or DELETE operations anywhere in the codebase | `[ ]` |
| 7.5 | `backup_runs` has no UPDATE operations (append-only audit record) | `[ ]` |
| 7.6 | Tauri updater HTTPS enforcement is enabled in production config | `[ ]` |
| 7.7 | Session token is in Tauri memory state, not in browser storage | `[ ]` |
| 7.8 | `cargo audit` reports 0 high-severity vulnerabilities in the dependency tree | `[ ]` |
| 7.9 | All bcrypt hashes use cost factor ≥ 12 (verified in `insertAdmin` scripts) | `[ ]` |

---

## Section 8 — Performance Baseline

| # | Item | Status |
|---|------|--------|
| 8.1 | Cold start (first launch, migrations pending): application window visible within 5 seconds | `[ ]` |
| 8.2 | Warm start (migrations applied): application window visible within 2 seconds | `[ ]` |
| 8.3 | Login round-trip (password check + session creation): completes within 1 second | `[ ]` |
| 8.4 | `generate_support_bundle` completes within 3 seconds (500 log lines) | `[ ]` |
| 8.5 | `run_manual_backup` for a ≤ 100 MB database completes within 10 seconds | `[ ]` |

---

## Section 9 — Build and Distribution

| # | Item | Status |
|---|------|--------|
| 9.1 | `pnpm run tauri build --target x86_64-pc-windows-msvc` succeeds | `[ ]` |
| 9.2 | The installed application starts and logs in from the built installer | `[ ]` |
| 9.3 | Log files are created in the correct platform-specific path after installation | `[ ]` |
| 9.4 | Uninstall removes the application binary but leaves the user data directory intact | `[ ]` |

---

## Section 10 — Known Phase 1 Limitations (Accepted)

The following items are intentionally deferred to Phase 2. They are listed here so the
Phase 2 team is aware of the open scope:

| # | Item | Deferral Rationale |
|---|------|--------------------|
| L1 | AES-256 backup encryption | Requires key management infrastructure (Phase 2 VPS) |
| L2 | Scheduled backup runner | Requires background task scheduler (Phase 2) |
| L3 | Factory reset (data deletion) | Requires VPS sync drain before safe deletion |
| L4 | Restore (live DB replacement) | Requires VPS sync drain before safe replacement |
| L5 | Updater signing key generation | Phase 2 DevOps CI/CD task |
| L6 | Live update manifest endpoint | Phase 2 infrastructure (hosting not yet provisioned) |
| L7 | Support ticket integration | Phase 2 feature (SP15: In-App Documentation) |
| L8 | `adm.backup` dedicated permission | Phase 2 RBAC extension |

---

## Sign-Off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Engineering Lead | | | |
| QA Lead | | | |
| Security Reviewer | | | |
| Product Owner | | | |

**Phase 1 is officially closed when all mandatory items are `[x]` and this document
is signed by all four roles.**
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- `docs/PHASE1_EXIT_CRITERIA.md` exists and contains all 10 sections
- Section count: ≥ 55 checkable items across sections 1–9
- All items default to `[ ]` (none pre-checked — this is a blank checklist)
- The 8 Phase 1 Limitations (L1–L8) are documented in Section 10
- The sign-off table has the four required roles
```

---

### Supervisor Verification — Sprint S3

**V1 — Exit criteria document is complete.**
Open `docs/PHASE1_EXIT_CRITERIA.md`. Verify that:
- All 10 sections exist (SP01–SP06 coverage + Security + Performance + Build + Limitations)
- Every item is individually numbered (e.g., 1.1, 4.12, 6.30)
- All items are in `[ ]` state (not pre-checked)
- The 8 known limitations table lists AES-256 encryption, scheduled backup, factory reset,
  restore live replacement, updater signing, live manifest, support tickets, and adm.backup

**V2 — Phase 1 limit deferrals are documented.**
Review Section 10. Each of the 8 limitations (L1–L8) must have a specific Phase 2
deferral rationale — not just "deferred to Phase 2" in general terms. The rationale
must explain WHY it cannot be done in Phase 1 (e.g., "Requires VPS sync drain").

**V3 — End-to-end backup preflight validation.**
Perform the following manual test sequence to validate the complete backup and restore
preflight flow:

1. Log in as admin
2. Perform step-up authentication
3. Run `run_manual_backup` via DevTools — confirm `status: "success"` and a non-empty `sha256_checksum`
4. Note the `output_path` from the result
5. Run `validate_backup_file` with that path — confirm `integrity_ok: true` and `checksum_match: true`
6. Attempt `validate_backup_file` with a path that does not exist — confirm a `NotFound` error
7. Paste the observed results in the Phase 1 exit checks table: items 6.24, 6.25, 6.26, 6.27 can be marked `[x]`

**V4 — Phase 1 closure readiness.**
Work through all items in `docs/PHASE1_EXIT_CRITERIA.md` sections 1–9 systematically.
Items that cannot be verified with a development build (e.g., 9.1 Windows installer build,
9.4 uninstall behavior) should be marked `[~]` with a note explaining the environment
constraint. After this review, at minimum:
- All SP06 items (6.1–6.30) should be `[x]`
- All SP04 items (4.1–4.12) should be `[x]`
- All SP05 items (5.1–5.8) should be `[x]`

Only when the checklist reflects the true state of the codebase can Phase 2 begin.

---

*End of Phase 1 · Sub-phase 06 · File 04 — Phase 1 Complete*

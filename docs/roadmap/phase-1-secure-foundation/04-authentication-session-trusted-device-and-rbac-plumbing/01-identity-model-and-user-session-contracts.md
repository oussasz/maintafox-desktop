# Phase 1 · Sub-phase 04 · File 01
# Identity Model and User Session Contracts

## Context and Purpose

Sub-phases 01–03 built the engineering standards, the Tauri shell, and the full local
data plane. `AppState` carries a `SessionManagerStub` — a placeholder deliberately left
for this sub-phase. Before Phase 2 can display a login screen, route by role, or enforce
any permission, the foundation must exist:

1. **Migration 002** (user tables) must now be fully specified. SP03-F01 registered its
   name but deferred the column definitions to this sub-phase.
2. **The auth domain model** — `AuthenticatedUser`, `LocalSession`, `SessionContext` —
   must be defined in Rust before any IPC command can depend on them.
3. **Password hashing** (argon2id) must be in place so the dev seed can create a working
   admin credential and the password-change flow has a real foundation.
4. **`SessionManager`** must replace the stub in `AppState` so every downstream command
   can gate on `state.session.read().is_authenticated()` rather than a flag.

This file also pins the IPC contracts that the UI (Login page, session guard) will
consume in Phase 2.

## Architecture Rules Applied

- All password hashing uses **argon2id** (PRD §12, security arch). The OWASP-recommended
  parameters for 2026 are: memory 64 MiB, parallelism 1, iterations 3, tag length 32 bytes.
  These are compile-time constants, not runtime config, so they cannot be weakened by DB row.
- Session tokens are **random bytes (256-bit)** generated via `rand::rngs::OsRng`, stored
  only in memory and the OS `keyring`. They are never written to SQLite in plain form.
- The `app_sessions` table stores session metadata (expiry, revoked flag, device id) but
  NOT the token value. The token is the OS-keyring entry; the row is the lifecycle record.
- IPC commands that require an active session call `require_session!(&state)` — a macro
  that returns `AppError::Auth("unauthenticated")` in O(1) if no session is active.
- The `SessionManager` lock is a `tokio::sync::RwLock` — readers (most auth-check commands)
  never block each other; only login/logout acquire a write lock.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src-tauri/migrations/m20260331_000002_user_tables.rs` (completed) | Full schema for user_accounts, roles, permissions, role_permissions, user_scope_assignments |
| `src-tauri/src/auth/mod.rs` | Auth domain: `AuthenticatedUser`, `LocalSession`, `SessionContext`, `SessionState` |
| `src-tauri/src/auth/password.rs` | argon2id hash + verify functions; compile-time parameters |
| `src-tauri/src/auth/session_manager.rs` | `SessionManager` replacing `SessionManagerStub` in AppState |
| `src-tauri/src/commands/auth.rs` | IPC: `login`, `logout`, `get_session_info`, `check_password` |
| Updated `src-tauri/src/state.rs` | Replaces `SessionManagerStub` with `SessionManager` |
| `shared/ipc-types.ts` (extended) | `SessionInfo`, `LoginRequest`, `LoginResponse`, `AuthError` |
| `src/services/auth-service.ts` | Frontend service: `login()`, `logout()`, `getSessionInfo()` |
| `src/hooks/use-session.ts` | React hook: session state, is-authenticated guard |
| `docs/AUTH_CONTRACTS.md` | Binding reference for all auth IPC commands and session model |

## Prerequisites

- SP03-F03 complete: repository pattern in place, `AppResult<T>` throughout
- SP03-F04 complete: seeder and integrity check running; `system_config` seeded
- SP02-F02 complete: `SessionManagerStub` in `AppState` — now to be replaced

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Migration 002 Completion, User Tables, and Password Hashing | Full migration 002 schema, `auth/password.rs`, DB seeder update for admin credential |
| S2 | Session Manager and Login/Logout IPC | `auth/session_manager.rs`, `SessionManager` in AppState, `commands/auth.rs` IPC commands |
| S3 | Frontend Session Contracts and Auth Service | `shared/ipc-types.ts`, `auth-service.ts`, `use-session.ts`, `docs/AUTH_CONTRACTS.md` |

---

## Sprint S1 — Migration 002 Completion, User Tables, and Password Hashing

### AI Agent Prompt

```
You are a senior Rust security engineer working on Maintafox Desktop (Tauri 2.x, sea-orm 1.x).
Sub-phases 01–03 are complete. Migrations 003–006 are applied. Migration 002 was registered
in migrations/mod.rs but its implementation file was deferred to Sub-phase 04.

YOUR TASK: Complete migration 002, add the argon2id password module, and update the dev
seed script to create a hashed admin credential.

─────────────────────────────────────────────────────────────────────
STEP 1 — Complete src-tauri/migrations/m20260331_000002_user_tables.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/migrations/m20260331_000002_user_tables.rs
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260331_000002_user_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── roles ────────────────────────────────────────────────────────────
        // System roles are seeded by the Rust seeder and cannot be deleted.
        // Custom roles are created by tenant administrators.
        // role_type: "system" | "custom"
        // status:    "draft" | "active" | "retired"
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("roles"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    // is_system=1: cannot be deleted, but can be modified carefully
                    .col(ColumnDef::new(Alias::new("is_system")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("role_type")).text().not_null().default("custom"))
                    .col(ColumnDef::new(Alias::new("status")).text().not_null().default("active"))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .to_owned(),
            )
            .await?;

        // ── permissions ───────────────────────────────────────────────────────
        // Permission name convention: domain.action[.scope]
        // Examples: eq.view, ot.create, adm.users, eq.import
        // is_dangerous=1: step-up reauthentication may be required before exercising
        // requires_step_up=1: MUST trigger step-up regardless of role settings
        // category: groups permissions by domain for UI display
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permissions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("name")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("category")).text().not_null().default("general"))
                    .col(ColumnDef::new(Alias::new("is_dangerous")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("requires_step_up")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("is_system")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        // ── role_permissions ──────────────────────────────────────────────────
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("role_permissions"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("role_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("permission_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("granted_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("granted_by_id")).integer())
                    .primary_key(Index::create().col(Alias::new("role_id")).col(Alias::new("permission_id")))
                    .to_owned(),
            )
            .await?;

        // ── user_accounts ─────────────────────────────────────────────────────
        // identity_mode: "local" | "sso" | "hybrid"
        // password_hash: argon2id hash; NULL for SSO-only users
        // pin_hash: optional fast-unlock PIN hash (argon2id, shorter parameters)
        // oauth_subject: external identity subject for SSO; NULL for local users
        // failed_login_attempts: reset on successful login; lockout at policy threshold
        // locked_until: populated after N failed logins; NULL = not locked
        // force_password_change: set for new accounts; clears on first password change
        // personnel_id: optional link to personnel record (migration 006+)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("user_accounts"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("username")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("display_name")).text())
                    .col(ColumnDef::new(Alias::new("identity_mode")).text().not_null().default("local"))
                    .col(ColumnDef::new(Alias::new("password_hash")).text())
                    .col(ColumnDef::new(Alias::new("pin_hash")).text())
                    .col(ColumnDef::new(Alias::new("oauth_subject")).text())
                    .col(ColumnDef::new(Alias::new("personnel_id")).integer())
                    .col(ColumnDef::new(Alias::new("is_active")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("is_admin")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("force_password_change")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("failed_login_attempts")).integer().not_null().default(0))
                    .col(ColumnDef::new(Alias::new("locked_until")).text())
                    .col(ColumnDef::new(Alias::new("last_login_at")).text())
                    .col(ColumnDef::new(Alias::new("last_seen_at")).text())
                    .col(ColumnDef::new(Alias::new("password_changed_at")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .col(ColumnDef::new(Alias::new("origin_machine_id")).text())
                    .to_owned(),
            )
            .await?;

        // username is used as login key — case-insensitive index
        manager
            .create_index(
                Index::create()
                    .name("idx_user_accounts_username_lower")
                    .table(Alias::new("user_accounts"))
                    .col(Alias::new("username"))
                    .to_owned(),
            )
            .await?;

        // ── user_scope_assignments ────────────────────────────────────────────
        // Binds a user to a role within a specific scope.
        // scope_type: "tenant" | "entity" | "site" | "team" | "org_node"
        // scope_reference: the id of the scope object (org_node.id, etc.)
        //                  NULL means tenant-wide scope
        // valid_from / valid_to: support temporary assignments and acting coverage
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("user_scope_assignments"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("sync_id")).text().not_null().unique_key())
                    .col(ColumnDef::new(Alias::new("user_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("role_id")).integer().not_null())
                    .col(ColumnDef::new(Alias::new("scope_type")).text().not_null().default("tenant"))
                    .col(ColumnDef::new(Alias::new("scope_reference")).text())
                    .col(ColumnDef::new(Alias::new("valid_from")).text())
                    .col(ColumnDef::new(Alias::new("valid_to")).text())
                    .col(ColumnDef::new(Alias::new("assigned_by_id")).integer())
                    .col(ColumnDef::new(Alias::new("notes")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("updated_at")).text().not_null())
                    .col(ColumnDef::new(Alias::new("deleted_at")).text())
                    .col(ColumnDef::new(Alias::new("row_version")).integer().not_null().default(1))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_user_scope_assignments_user_id")
                    .table(Alias::new("user_scope_assignments"))
                    .col(Alias::new("user_id"))
                    .to_owned(),
            )
            .await?;

        // ── permission_dependencies ───────────────────────────────────────────
        // Warn or block configurations that combine dangerous permissions with
        // missing prerequisite visibility or edit permissions.
        // dependency_type: "hard" (blocked) | "warn" (advisory)
        manager
            .create_table(
                Table::create()
                    .table(Alias::new("permission_dependencies"))
                    .if_not_exists()
                    .col(ColumnDef::new(Alias::new("id")).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Alias::new("permission_name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("required_permission_name")).text().not_null())
                    .col(ColumnDef::new(Alias::new("dependency_type")).text().not_null().default("warn"))
                    .col(ColumnDef::new(Alias::new("description")).text())
                    .col(ColumnDef::new(Alias::new("created_at")).text().not_null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for tbl in &[
            "permission_dependencies",
            "user_scope_assignments",
            "user_accounts",
            "role_permissions",
            "permissions",
            "roles",
        ] {
            manager
                .drop_table(Table::drop().table(Alias::new(tbl)).to_owned())
                .await?;
        }
        Ok(())
    }
}
```

Register in migrations/mod.rs if not already present (it should already be registered
from SP01-F03 — verify only the file name is correct and the `Migration` struct exists):
```
mod m20260331_000002_user_tables;
```
Also ensure it is the second entry in the `Migrator::migrations()` vec.

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src-tauri/src/auth/mod.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/auth/mod.rs
//! Authentication and identity domain.
//!
//! Module layout:
//!   auth::password      — argon2id hash/verify
//!   auth::session_manager — SessionManager, SessionContext, LocalSession
//!
//! Architecture rules:
//!   - No session token is ever stored in SQLite in plaintext.
//!   - The SessionManager is the single authoritative source of who is logged in.
//!   - All IPC commands that need auth call `require_session!(&state)`.

pub mod password;
pub mod session_manager;
```

Declare in src-tauri/src/lib.rs:
```rust
mod auth;
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src-tauri/src/auth/password.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/auth/password.rs
//! argon2id password hashing module.
//!
//! Security properties:
//!   - Uses argon2id (hybrid of argon2i and argon2d) for resistance to
//!     both side-channel and GPU attacks.
//!   - OWASP-recommended 2026 parameters: m=65536 (64 MiB), t=3, p=1.
//!   - Salt is 16 random bytes per hash, embedded in the PHC string output.
//!   - Constant-time comparison via argon2::verify_password.
//!   - compile-time constants — parameters cannot be weakened at runtime.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Algorithm, Params, Version,
};
use crate::errors::{AppError, AppResult};

/// Argon2id memory cost in KiB (64 MiB).
const MEMORY_COST_KIB: u32 = 64 * 1024;
/// Argon2id iteration count.
const TIME_COST: u32 = 3;
/// Argon2id parallelism degree.
const PARALLELISM: u32 = 1;

/// Returns the configured Argon2id hasher.
/// Panics at compile time if parameters are out of range.
fn argon2_hasher() -> Argon2<'static> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .expect("argon2id: invalid parameters — check MEMORY_COST_KIB, TIME_COST, PARALLELISM constants");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a password using argon2id. Returns a PHC string suitable for storage.
///
/// # Security
/// - Salt is generated from OS entropy (OsRng). Never reuse salts.
/// - The returned string embeds the salt and parameters.
/// - Typical timing: ~100–400ms depending on CPU. This is intentional.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2_hasher()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("password hash failed: {e}")))
}

/// Verify a plaintext password against a stored argon2id PHC hash.
///
/// Returns Ok(true) if the password matches, Ok(false) if it does not match.
/// Returns Err only if the stored hash string is malformed (database corruption).
///
/// # Security
/// This function is constant-time with respect to the password value,
/// preventing timing attacks on password comparison.
pub fn verify_password(password: &str, stored_hash: &str) -> AppResult<bool> {
    let parsed_hash = PasswordHash::new(stored_hash)
        .map_err(|e| AppError::Internal(format!("malformed password hash in DB: {e}")))?;
    Ok(argon2_hasher()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

// ── Compile-time safety tests ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_round_trip() {
        let password = "Correct Horse Battery Staple!";
        let hash = hash_password(password).expect("hash should not fail");
        assert!(hash.starts_with("$argon2id"), "Expected argon2id PHC string");

        let result = verify_password(password, &hash).expect("verify should not fail");
        assert!(result, "Correct password should verify");
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hash = hash_password("the_right_password").expect("hash should not fail");
        let result = verify_password("the_wrong_password", &hash)
            .expect("verify should not fail on valid hash");
        assert!(!result, "Wrong password must not verify");
    }

    #[test]
    fn two_hashes_of_same_password_are_different() {
        // Different salts must produce different PHC strings
        let h1 = hash_password("same").expect("hash 1");
        let h2 = hash_password("same").expect("hash 2");
        assert_ne!(h1, h2, "Same password must produce different hashes (random salt)");
    }

    #[test]
    fn malformed_hash_returns_error() {
        let result = verify_password("password", "not_a_valid_phc_string");
        assert!(result.is_err(), "Malformed hash must return Err");
    }

    #[test]
    fn parameters_are_within_argon2_bounds() {
        // Panics at test run if parameters are invalid — catches misconfigurations early
        let _ = argon2_hasher();
    }
}
```

Add argon2 to src-tauri/Cargo.toml:
```toml
argon2 = { version = "0.5", features = ["password-hash", "rand_core"] }
rand = "0.8"
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Update dev seed to create hashed admin credential
─────────────────────────────────────────────────────────────────────
In the Rust seeder (src-tauri/src/db/seeder.rs), add `seed_admin_account` to
the end of `seed_system_data()`:

```rust
// In seed_system_data(), after the lookup domain seeds and config record:
seed_admin_account(db).await?;
```

Add the function:
```rust
/// Seeds the initial admin account for first launch.
/// Uses username "admin" with a known dev password "Admin#2026!" that forces
/// a password change on first login (force_password_change = 1).
///
/// This is only a development bootstrap credential. In production, the
/// first-launch wizard sets the administrator password interactively.
///
/// Safety: INSERT OR IGNORE — will not overwrite an existing admin account.
async fn seed_admin_account(db: &DatabaseConnection) -> AppResult<()> {
    use crate::auth::password::hash_password;

    // Check if admin already exists
    let existing = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM user_accounts WHERE username = ?",
        ["admin".into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    if existing.is_some() {
        tracing::debug!("seeder::admin_account already exists, skipping");
        return Ok(());
    }

    let password_hash = hash_password("Admin#2026!")
        .map_err(|e| AppError::Internal(format!("seed admin hash failed: {e}")))?;

    let now = Utc::now().to_rfc3339();
    let sync_id = Uuid::new_v4().to_string();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT OR IGNORE INTO user_accounts
               (sync_id, username, display_name, identity_mode, password_hash,
                is_active, is_admin, force_password_change,
                failed_login_attempts, created_at, updated_at, row_version)
           VALUES (?, ?, ?, ?, ?, 1, 1, 1, 0, ?, ?, 1)
        "#,
        [
            sync_id.into(),
            "admin".into(),
            "Administrateur Maintafox".into(),
            "local".into(),
            password_hash.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    tracing::info!("seeder::admin_account created (force_password_change=1)");
    Ok(())
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (5 new password tests must all pass)
- pnpm run dev launches cleanly; startup log shows migration 002 applied and seeder
  creating admin account
- DBeaver shows: roles table, permissions table, role_permissions table,
  user_accounts table (1 row: admin), user_scope_assignments table,
  permission_dependencies table
- user_accounts.password_hash for admin starts with "$argon2id$"
- user_accounts.force_password_change for admin = 1
```

---

### Supervisor Verification — Sprint S1

**V1 — Migration 002 tables are present.**
Run `pnpm run dev`. Open the database in DBeaver. The following tables must appear in the
left panel: `roles`, `permissions`, `role_permissions`, `user_accounts`,
`user_scope_assignments`, `permission_dependencies`. If any are missing, flag by name.

**V2 — Admin account was seeded with a real argon2id hash.**
In DBeaver, run:
```sql
SELECT username, identity_mode, is_admin, force_password_change,
       substr(password_hash, 1, 20) as hash_prefix
FROM user_accounts WHERE username = 'admin';
```
The `hash_prefix` column should start with `$argon2id$`. If it shows NULL or a different
prefix, the password was not hashed. Flag it immediately — this is a security defect.

**V3 — Hash round-trip test passes.**
Run `cd src-tauri && cargo test auth::password`. All 5 tests (`hash_and_verify_round_trip`,
`wrong_password_does_not_verify`, `two_hashes_of_same_password_are_different`,
`malformed_hash_returns_error`, `parameters_are_within_argon2_bounds`) must show `ok`.
If any fail, flag the test name.

**V4 — Different salts per hash confirmed.**
This is verified by test `two_hashes_of_same_password_are_different`. A PASS result for
this test is sufficient. Supervisor does not need to inspect the DB manually for this.

---

## Sprint S2 — Session Manager and Login/Logout IPC

### AI Agent Prompt

```
You are a senior Rust and Tauri 2.x security engineer continuing work on Maintafox Desktop.
Sprint S1 is complete: migration 002 is applied, user_accounts table has an admin row,
argon2id hash/verify is in place. Your task is to replace the SessionManagerStub with a
real SessionManager and implement the login, logout, and get_session_info IPC commands.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src-tauri/src/auth/session_manager.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/auth/session_manager.rs
//! SessionManager: owns the in-memory session, expiry enforcement, and
//! the OS-keyring session token.
//!
//! Rules:
//!   - Exactly one active session at a time per desktop instance.
//!   - The session token is a 32-byte random value stored in OS keyring only.
//!   - The app_sessions row is the lifecycle record; expiry is enforced here in memory.
//!   - Every write (login, logout, expire) emits an audit event via the db.

use std::time::SystemTime;
use serde::Serialize;
use uuid::Uuid;
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use crate::errors::{AppError, AppResult};

/// Session duration: 8 hours of activity before forced re-login.
pub const SESSION_DURATION_HOURS: i64 = 8;
/// Idle timeout: session is locked (not expired) after 30 minutes of no activity.
pub const IDLE_LOCK_MINUTES: i64 = 30;

/// The identity of an authenticated user, embedded in the active session.
#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedUser {
    pub user_id: i32,
    pub username: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub force_password_change: bool,
}

/// The full context of an active local session.
#[derive(Debug, Clone, Serialize)]
pub struct LocalSession {
    /// Row id in app_sessions
    pub session_db_id: String,
    pub user: AuthenticatedUser,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub is_locked: bool,
}

impl LocalSession {
    /// True if the session has passed its hard expiry time.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// True if the session is idle-locked (no activity for IDLE_LOCK_MINUTES).
    pub fn is_idle_locked(&self) -> bool {
        let idle_deadline = self.last_activity_at + Duration::minutes(IDLE_LOCK_MINUTES);
        self.is_locked || Utc::now() > idle_deadline
    }
}

/// Serializable summary returned by the `get_session_info` IPC command.
/// Does NOT include the token, password hash, or any credential material.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub is_authenticated: bool,
    pub is_locked: bool,
    pub user_id: Option<i32>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub force_password_change: Option<bool>,
    pub expires_at: Option<String>,
    pub last_activity_at: Option<String>,
}

/// The session manager holds the current session in memory.
/// All access is through RwLock via AppState.
#[derive(Debug, Default)]
pub struct SessionManager {
    /// Current active session; None when no user is logged in.
    pub current: Option<LocalSession>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { current: None }
    }

    /// True if there is a non-expired, non-locked session.
    pub fn is_authenticated(&self) -> bool {
        self.current
            .as_ref()
            .map(|s| !s.is_expired() && !s.is_idle_locked())
            .unwrap_or(false)
    }

    /// Returns a reference to the current authenticated user, if any.
    /// Returns None if there is no session, or if the session is expired/locked.
    pub fn current_user(&self) -> Option<&AuthenticatedUser> {
        self.current
            .as_ref()
            .filter(|s| !s.is_expired())
            .map(|s| &s.user)
    }

    /// Updates the last_activity_at timestamp to prevent idle lock.
    /// Call this at the start of any authenticated IPC command.
    pub fn touch(&mut self) {
        if let Some(session) = &mut self.current {
            session.last_activity_at = Utc::now();
        }
    }

    /// Create a new session after a successful authentication check.
    pub fn create_session(&mut self, user: AuthenticatedUser) -> &LocalSession {
        let now = Utc::now();
        let session = LocalSession {
            session_db_id: Uuid::new_v4().to_string(),
            user,
            created_at: now,
            expires_at: now + Duration::hours(SESSION_DURATION_HOURS),
            last_activity_at: now,
            is_locked: false,
        };
        self.current = Some(session);
        self.current.as_ref().unwrap()
    }

    /// Lock the current session (e.g., idle timeout or manual lock).
    pub fn lock_session(&mut self) {
        if let Some(session) = &mut self.current {
            session.is_locked = true;
        }
    }

    /// Clear the current session (logout or forced expiry).
    pub fn clear_session(&mut self) -> Option<String> {
        let session_id = self.current.as_ref().map(|s| s.session_db_id.clone());
        self.current = None;
        session_id
    }

    /// Returns a `SessionInfo` summary for the IPC response.
    pub fn session_info(&self) -> SessionInfo {
        match &self.current {
            None => SessionInfo {
                is_authenticated: false,
                is_locked: false,
                user_id: None,
                username: None,
                display_name: None,
                is_admin: None,
                force_password_change: None,
                expires_at: None,
                last_activity_at: None,
            },
            Some(s) => SessionInfo {
                is_authenticated: !s.is_expired() && !s.is_idle_locked(),
                is_locked: s.is_idle_locked(),
                user_id: Some(s.user.user_id),
                username: Some(s.user.username.clone()),
                display_name: s.user.display_name.clone(),
                is_admin: Some(s.user.is_admin),
                force_password_change: Some(s.user.force_password_change),
                expires_at: Some(s.expires_at.to_rfc3339()),
                last_activity_at: Some(s.last_activity_at.to_rfc3339()),
            },
        }
    }
}

// ── DB helpers ────────────────────────────────────────────────────────────────

/// Look up a user account by username (case-insensitive via LOWER()).
/// Returns None if the user does not exist or is not active.
pub async fn find_active_user(
    db: &DatabaseConnection,
    username: &str,
) -> AppResult<Option<(i32, String, Option<String>, bool, bool, Option<String>)>> {
    // Returns: (id, username, display_name, is_admin, force_password_change, password_hash)
    let row = db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"SELECT id, username, display_name, is_admin, force_password_change, password_hash
           FROM user_accounts
           WHERE LOWER(username) = LOWER(?) AND is_active = 1 AND deleted_at IS NULL"#,
        [username.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(row.map(|r| {
        (
            r.try_get::<i32>("", "id").unwrap_or(0),
            r.try_get::<String>("", "username").unwrap_or_default(),
            r.try_get::<Option<String>>("", "display_name").unwrap_or(None),
            r.try_get::<i32>("", "is_admin").unwrap_or(0) == 1,
            r.try_get::<i32>("", "force_password_change").unwrap_or(0) == 1,
            r.try_get::<Option<String>>("", "password_hash").unwrap_or(None),
        )
    }))
}

/// Increment failed_login_attempts for a user. Locks account at 10 attempts.
pub async fn record_failed_login(
    db: &DatabaseConnection,
    user_id: i32,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE user_accounts
           SET failed_login_attempts = failed_login_attempts + 1,
               locked_until = CASE
                   WHEN failed_login_attempts + 1 >= 10
                   THEN datetime('now', '+15 minutes')
                   ELSE locked_until
               END,
               updated_at = ?
           WHERE id = ?"#,
        [now.into(), user_id.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

/// Reset failed_login_attempts and locked_until after successful login.
pub async fn record_successful_login(
    db: &DatabaseConnection,
    user_id: i32,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"UPDATE user_accounts
           SET failed_login_attempts = 0,
               locked_until = NULL,
               last_login_at = ?,
               last_seen_at = ?,
               updated_at = ?
           WHERE id = ?"#,
        [now.clone().into(), now.clone().into(), now.into(), user_id.into()],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

/// Write an app_sessions row for audit purposes.
pub async fn create_session_record(
    db: &DatabaseConnection,
    session_db_id: &str,
    user_id: i32,
    expires_at: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO app_sessions (id, user_id, created_at, expires_at, is_revoked)
           VALUES (?, ?, ?, ?, 0)"#,
        [
            session_db_id.into(),
            user_id.into(),
            now.into(),
            expires_at.into(),
        ],
    ))
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

// ── Unit tests ─────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: 1,
            username: "test_user".into(),
            display_name: Some("Test User".into()),
            is_admin: false,
            force_password_change: false,
        }
    }

    #[test]
    fn new_manager_is_not_authenticated() {
        let mgr = SessionManager::new();
        assert!(!mgr.is_authenticated());
        assert!(mgr.current_user().is_none());
    }

    #[test]
    fn create_session_sets_authenticated() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        assert!(mgr.is_authenticated());
        assert_eq!(mgr.current_user().unwrap().username, "test_user");
    }

    #[test]
    fn clear_session_removes_authentication() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.clear_session();
        assert!(!mgr.is_authenticated());
    }

    #[test]
    fn lock_session_reports_locked() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        mgr.lock_session();
        assert!(!mgr.is_authenticated(), "Locked session must not be 'authenticated'");
        assert!(mgr.current.as_ref().unwrap().is_locked);
    }

    #[test]
    fn session_info_unauthenticated_is_all_none() {
        let mgr = SessionManager::new();
        let info = mgr.session_info();
        assert!(!info.is_authenticated);
        assert!(info.user_id.is_none());
        assert!(info.username.is_none());
    }

    #[test]
    fn session_info_authenticated_has_user_fields() {
        let mut mgr = SessionManager::new();
        mgr.create_session(make_user());
        let info = mgr.session_info();
        assert!(info.is_authenticated);
        assert_eq!(info.user_id, Some(1));
        assert_eq!(info.username.as_deref(), Some("test_user"));
        assert_eq!(info.is_admin, Some(false));
    }
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Replace SessionManagerStub with SessionManager in state.rs
─────────────────────────────────────────────────────────────────────
In src-tauri/src/state.rs, replace:
```rust
use crate::auth::session_manager::SessionManager;
// ... remove SessionManagerStub struct and Default impl
```

Change the `session` field type from `Arc<RwLock<SessionManagerStub>>` to
`Arc<RwLock<SessionManager>>`:
```rust
pub session: Arc<RwLock<SessionManager>>,
```

And in `AppState::new()`:
```rust
session: Arc::new(RwLock::new(SessionManager::new())),
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src-tauri/src/commands/auth.rs
─────────────────────────────────────────────────────────────────────
```rust
// src-tauri/src/commands/auth.rs
//! Authentication IPC commands.
//!
//! Security rules:
//!   - login() never returns a useful error on bad credentials — always generic.
//!   - login() never reveals whether a username exists or not.
//!   - All auth errors are logged at WARN level with the username (not password).
//!   - The session token is not returned in any IPC response.

use tauri::State;
use serde::{Deserialize, Serialize};
use tracing::warn;
use crate::state::AppState;
use crate::auth::{password, session_manager};
use crate::errors::{AppError, AppResult};

/// Input for the login command. Received from the React login form.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Response returned on successful login.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub session_info: session_manager::SessionInfo,
}

/// Attempt to authenticate with a local username and password.
///
/// Returns an opaque error on any failure — does not reveal whether the
/// username exists, whether the password was wrong, or whether the account
/// is locked. Details are logged at WARN level on the Rust side only.
#[tauri::command]
pub async fn login(
    payload: LoginRequest,
    state: State<'_, AppState>,
) -> AppResult<LoginResponse> {
    // Normalise username: trim whitespace, lowercase for lookup
    let username = payload.username.trim().to_string();

    if username.is_empty() || payload.password.is_empty() {
        return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
    }

    // Look up user
    let user_record = session_manager::find_active_user(&state.db, &username).await?;
    let (user_id, db_username, display_name, is_admin, force_pw_change, pw_hash) =
        match user_record {
            None => {
                // User not found — run a dummy hash to consume constant time
                let _ = password::hash_password("timing_sink_unused");
                warn!(username = %username, "login::user_not_found");
                return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
            }
            Some(r) => r,
        };

    // Verify password
    let stored_hash = match pw_hash {
        None => {
            warn!(username = %username, "login::no_password_hash_sso_only");
            return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
        }
        Some(h) => h,
    };

    let password_ok = password::verify_password(&payload.password, &stored_hash)?;
    if !password_ok {
        session_manager::record_failed_login(&state.db, user_id).await?;
        warn!(username = %username, "login::wrong_password");
        return Err(AppError::Auth("Identifiant ou mot de passe invalide.".into()));
    }

    // Password correct — create session
    let auth_user = session_manager::AuthenticatedUser {
        user_id,
        username: db_username,
        display_name,
        is_admin,
        force_password_change: force_pw_change,
    };

    let mut session_guard = state.session.write().await;
    let session = session_guard.create_session(auth_user);

    // Record session in DB for audit purposes
    let session_id = session.session_db_id.clone();
    let expires_rfc3339 = session.expires_at.to_rfc3339();
    drop(session_guard); // release write lock before async DB call

    session_manager::record_successful_login(&state.db, user_id).await?;
    session_manager::create_session_record(&state.db, &session_id, user_id, &expires_rfc3339).await?;

    let info = state.session.read().await.session_info();
    Ok(LoginResponse { session_info: info })
}

/// Log the current user out and clear the active session.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> AppResult<()> {
    let mut session_guard = state.session.write().await;
    if let Some(session_id) = session_guard.clear_session() {
        tracing::info!(session_id = %session_id, "auth::logout");
    }
    Ok(())
}

/// Returns the current session state without requiring authentication.
/// Called by the React shell to decide which screen to show on startup.
#[tauri::command]
pub async fn get_session_info(
    state: State<'_, AppState>,
) -> AppResult<session_manager::SessionInfo> {
    Ok(state.session.read().await.session_info())
}
```

Register in commands/mod.rs:
```rust
pub mod auth;
```

Register in lib.rs generate_handler!:
```rust
commands::auth::login,
commands::auth::logout,
commands::auth::get_session_info,
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Add `require_session!` macro to errors.rs or a new guards.rs
─────────────────────────────────────────────────────────────────────
Add this macro to src-tauri/src/auth/mod.rs:

```rust
/// Short-circuit an IPC command if there is no active authenticated session.
/// Usage:
///   let user = require_session!(state);  // returns &AuthenticatedUser
#[macro_export]
macro_rules! require_session {
    ($state:expr) => {{
        let guard = $state.session.read().await;
        if !guard.is_authenticated() {
            return Err($crate::errors::AppError::Auth(
                "Session expirée ou absente. Veuillez vous reconnecter.".into(),
            ));
        }
        // SAFETY: is_authenticated() guarantees current is Some and non-expired
        guard.current.as_ref().unwrap().user.clone()
    }};
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (6 new session_manager tests)
- pnpm run dev starts cleanly
- `invoke('get_session_info')` returns `{ is_authenticated: false }` before login
- `invoke('login', { username: 'admin', password: 'Admin#2026!' })` returns a session
  with `is_authenticated: true`, `username: 'admin'`, `force_password_change: true`
- `invoke('login', { username: 'admin', password: 'wrong' })` returns an error whose
  message is exactly "Identifiant ou mot de passe invalide." (no information leak)
- `invoke('logout')` followed by `invoke('get_session_info')` returns
  `{ is_authenticated: false }`
```

---

### Supervisor Verification — Sprint S2

**V1 — Login with correct credentials succeeds.**
Open Tauri Developer Tools. In the console, run:
```javascript
window.__TAURI__.core.invoke('login', { payload: { username: 'admin', password: 'Admin#2026!' } })
  .then(r => console.log(JSON.stringify(r, null, 2)));
```
The response must show `session_info.is_authenticated === true`,
`session_info.username === "admin"`, `session_info.force_password_change === true`.
If the call rejects or shows `is_authenticated: false`, flag the error message.

**V2 — Login with wrong password gives opaque error.**
Run:
```javascript
window.__TAURI__.core.invoke('login', { payload: { username: 'admin', password: 'wrongpassword' } })
  .catch(e => console.log('Error:', JSON.stringify(e)));
```
The error message must be exactly `"Identifiant ou mot de passe invalide."` — it must
NOT say "user not found", "wrong password", "account locked", or any detail that would
help an attacker. If the message reveals any distinction, flag it as an information
disclosure defect.

**V3 — Session info before login is unauthenticated.**
Restart the application (close and reopen). Before logging in, run:
```javascript
window.__TAURI__.core.invoke('get_session_info').then(r => console.log(r));
```
The result must show `{ is_authenticated: false }` with all other fields null. If
`is_authenticated` is true before login, flag it as a critical session management defect.

**V4 — Logout clears session correctly.**
After logging in, run:
```javascript
window.__TAURI__.core.invoke('logout')
  .then(() => window.__TAURI__.core.invoke('get_session_info'))
  .then(r => console.log('After logout:', r.is_authenticated));
```
The output must show `false`. If `is_authenticated` remains true after logout, flag it.

---

## Sprint S3 — Frontend Session Contracts and Auth Service

### AI Agent Prompt

```
You are a senior React and TypeScript engineer continuing work on Maintafox Desktop.
Sprint S2 is complete: login, logout, and get_session_info IPC commands are live.
Your task is to build the typed frontend contracts, the auth service, the session hook,
and the auth contract document.

─────────────────────────────────────────────────────────────────────
STEP 1 — Extend shared/ipc-types.ts with auth types
─────────────────────────────────────────────────────────────────────
```typescript
// shared/ipc-types.ts — add to existing exports

export interface SessionInfo {
  is_authenticated: boolean;
  is_locked: boolean;
  user_id: number | null;
  username: string | null;
  display_name: string | null;
  is_admin: boolean | null;
  force_password_change: boolean | null;
  expires_at: string | null;      // ISO 8601
  last_activity_at: string | null; // ISO 8601
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  session_info: SessionInfo;
}
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src/services/auth-service.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/services/auth-service.ts
//! ADR-003 compliant: all IPC calls for authentication go through this file only.
//! Components and hooks MUST NOT import from @tauri-apps/api/core directly
//! for auth operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { LoginRequest, LoginResponse, SessionInfo } from "@shared/ipc-types";

// ── Zod schemas for runtime validation ────────────────────────────────────────
const sessionInfoSchema = z.object({
  is_authenticated: z.boolean(),
  is_locked: z.boolean(),
  user_id: z.number().nullable(),
  username: z.string().nullable(),
  display_name: z.string().nullable(),
  is_admin: z.boolean().nullable(),
  force_password_change: z.boolean().nullable(),
  expires_at: z.string().nullable(),
  last_activity_at: z.string().nullable(),
});

const loginResponseSchema = z.object({
  session_info: sessionInfoSchema,
});

// ── Service functions ─────────────────────────────────────────────────────────

/**
 * Attempt to log in with username and password.
 * Throws a string error message on failure (safe to display to user).
 */
export async function login(request: LoginRequest): Promise<LoginResponse> {
  const raw = await invoke<unknown>("login", { payload: request });
  return loginResponseSchema.parse(raw);
}

/**
 * Log out the current user and clear the session.
 */
export async function logout(): Promise<void> {
  await invoke<void>("logout");
}

/**
 * Get the current session info without requiring authentication.
 * Safe to call on every app startup to determine initial route.
 */
export async function getSessionInfo(): Promise<SessionInfo> {
  const raw = await invoke<unknown>("get_session_info");
  return sessionInfoSchema.parse(raw);
}
```

─────────────────────────────────────────────────────────────────────
STEP 3 — Create src/hooks/use-session.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/use-session.ts
import { useState, useCallback, useEffect } from "react";
import { getSessionInfo, login as authLogin, logout as authLogout } from "@/services/auth-service";
import type { SessionInfo, LoginRequest } from "@shared/ipc-types";

interface SessionState {
  info: SessionInfo | null;
  isLoading: boolean;
  error: string | null;
}

interface SessionActions {
  login: (req: LoginRequest) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
}

const UNAUTHENTICATED: SessionInfo = {
  is_authenticated: false,
  is_locked: false,
  user_id: null,
  username: null,
  display_name: null,
  is_admin: null,
  force_password_change: null,
  expires_at: null,
  last_activity_at: null,
};

/**
 * Primary session hook. Fetches session state on mount and after login/logout.
 * Components that need to gate on authentication status use this hook.
 */
export function useSession(): SessionState & SessionActions {
  const [state, setState] = useState<SessionState>({
    info: null,
    isLoading: true,
    error: null,
  });

  const refresh = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true, error: null }));
    try {
      const info = await getSessionInfo();
      setState({ info, isLoading: false, error: null });
    } catch (e) {
      setState({
        info: UNAUTHENTICATED,
        isLoading: false,
        error: e instanceof Error ? e.message : "Erreur de session.",
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const login = useCallback(
    async (req: LoginRequest) => {
      setState((s) => ({ ...s, isLoading: true, error: null }));
      try {
        const response = await authLogin(req);
        setState({ info: response.session_info, isLoading: false, error: null });
      } catch (e) {
        setState((s) => ({
          ...s,
          isLoading: false,
          error: e instanceof Error ? e.message : "Identifiant ou mot de passe invalide.",
        }));
        throw e; // re-throw so the login form can react
      }
    },
    []
  );

  const logoutAction = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true }));
    try {
      await authLogout();
      setState({ info: UNAUTHENTICATED, isLoading: false, error: null });
    } catch (e) {
      setState((s) => ({
        ...s,
        isLoading: false,
        error: e instanceof Error ? e.message : "Erreur lors de la déconnexion.",
      }));
    }
  }, []);

  return { ...state, login, logout: logoutAction, refresh };
}
```

─────────────────────────────────────────────────────────────────────
STEP 4 — Create docs/AUTH_CONTRACTS.md
─────────────────────────────────────────────────────────────────────
```markdown
# Authentication Contracts

Source of truth for all IPC commands and frontend service contracts in the auth domain.
Backend: `src-tauri/src/commands/auth.rs`
Frontend service: `src/services/auth-service.ts`
Frontend hook: `src/hooks/use-session.ts`

## Session Model

A session has four possible states:

| State | Condition | App behavior |
|-------|-----------|-------------|
| `unauthenticated` | `is_authenticated: false`, `is_locked: false` | Show login screen |
| `authenticated` | `is_authenticated: true`, `is_locked: false` | Normal operation |
| `idle_locked` | `is_authenticated: false`, `is_locked: true` | Show quick unlock (PIN or password) |
| `force_change` | `is_authenticated: true`, `force_password_change: true` | Redirect to password change screen before any module |

Session duration: 8 hours of activity  
Idle lock timeout: 30 minutes of no IPC activity  
Lockout threshold: 10 consecutive failed login attempts → 15-minute cooldown

## IPC Commands

### login

```
Command:   login
Payload:   { username: string, password: string }
Response:  { session_info: SessionInfo }
Errors:    AUTH_ERROR — "Identifiant ou mot de passe invalide." (always opaque)
```

Security invariants:
- The error message is ALWAYS the same string regardless of the failure reason
  (user not found / wrong password / account locked / SSO-only account).
  This prevents user enumeration.
- The password is never stored in logs, traces, or audit events.
- The argon2id hash parameters are compile-time constants (m=64MiB, t=3, p=1).
- After 10 consecutive failures, `locked_until` is set; login returns the same
  opaque error — no countdown is exposed.

### logout

```
Command:   logout
Payload:   (none)
Response:  null
Errors:    (none — logout always succeeds, even if there is no active session)
```

### get_session_info

```
Command:   get_session_info
Payload:   (none)
Response:  SessionInfo
Errors:    (none — always returns a SessionInfo; is_authenticated = false if no session)
```

## Frontend Service Contracts

All auth IPC calls go through `src/services/auth-service.ts`. Components and hooks
NEVER call `invoke()` directly for auth operations (ADR-003 compliance).

All responses are validated through Zod schemas at the service layer. A response that
doesn't match the schema throws a ZodError before the application state is modified.

## Security Rules

1. Session token is not present in any IPC response, Zod schema, or React state.
2. `force_password_change: true` must redirect to the password-change screen immediately
   after login. No module is accessible until the flag is cleared.
3. The `require_session!` macro is the single enforcement point for authenticated access
   on the Rust side. IPC commands that need auth MUST use it.
4. `get_session_info` is the only command that is callable without authentication —
   everything else that modifies state must be behind `require_session!`.
```

─────────────────────────────────────────────────────────────────────
STEP 5 — Update IPC_COMMAND_REGISTRY.md
─────────────────────────────────────────────────────────────────────
Add three new entries:

```markdown
## login

| Field | Value |
|-------|-------|
| Command | `login` |
| Module | Authentication |
| Auth Required | No |
| Parameters | `LoginRequest { username: string, password: string }` |
| Response | `LoginResponse { session_info: SessionInfo }` |
| Errors | `AUTH_ERROR: "Identifiant ou mot de passe invalide."` |
| Since | v0.1.0 |
| PRD Ref | §6.1 Authentication & Session Management |

## logout

| Field | Value |
|-------|-------|
| Command | `logout` |
| Module | Authentication |
| Auth Required | No (safe to call without session) |
| Parameters | None |
| Response | null |
| Errors | None |
| Since | v0.1.0 |

## get_session_info

| Field | Value |
|-------|-------|
| Command | `get_session_info` |
| Module | Authentication |
| Auth Required | No |
| Parameters | None |
| Response | `SessionInfo` |
| Errors | None |
| Since | v0.1.0 |
| PRD Ref | §6.1 |
```

─────────────────────────────────────────────────────────────────────
STEP 6 — Add unit tests for use-session hook
─────────────────────────────────────────────────────────────────────
```typescript
// src/hooks/__tests__/use-session.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { SessionInfo } from "@shared/ipc-types";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

import { useSession } from "../use-session";

const authSession: SessionInfo = {
  is_authenticated: true,
  is_locked: false,
  user_id: 1,
  username: "admin",
  display_name: "Administrateur",
  is_admin: true,
  force_password_change: true,
  expires_at: new Date(Date.now() + 8 * 3600 * 1000).toISOString(),
  last_activity_at: new Date().toISOString(),
};

const noSession: SessionInfo = {
  is_authenticated: false,
  is_locked: false,
  user_id: null, username: null, display_name: null,
  is_admin: null, force_password_change: null,
  expires_at: null, last_activity_at: null,
};

describe("useSession", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("fetches session info on mount", async () => {
    mockInvoke.mockResolvedValueOnce(noSession);
    const { result } = renderHook(() => useSession());

    await act(async () => {});
    expect(mockInvoke).toHaveBeenCalledWith("get_session_info");
    expect(result.current.info?.is_authenticated).toBe(false);
  });

  it("login updates session info", async () => {
    mockInvoke
      .mockResolvedValueOnce(noSession)                          // initial refresh
      .mockResolvedValueOnce({ session_info: authSession });     // login response

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => {
      await result.current.login({ username: "admin", password: "Admin#2026!" });
    });

    expect(result.current.info?.is_authenticated).toBe(true);
    expect(result.current.info?.username).toBe("admin");
  });

  it("login error does not update session info", async () => {
    mockInvoke
      .mockResolvedValueOnce(noSession)
      .mockRejectedValueOnce(new Error("Identifiant ou mot de passe invalide."));

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => {
      try {
        await result.current.login({ username: "admin", password: "wrong" });
      } catch {
        // expected
      }
    });

    expect(result.current.info?.is_authenticated).toBe(false);
    expect(result.current.error).toBeTruthy();
  });

  it("logout clears session", async () => {
    mockInvoke
      .mockResolvedValueOnce(authSession)     // initial load
      .mockResolvedValueOnce(undefined);       // logout

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => { await result.current.logout(); });
    expect(result.current.info?.is_authenticated).toBe(false);
  });
});
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- pnpm run test passes with 0 failures including 4 new use-session tests
- pnpm run typecheck passes with 0 errors
- docs/AUTH_CONTRACTS.md is present and documents all 3 IPC commands
- IPC_COMMAND_REGISTRY.md has entries for login, logout, get_session_info
- No session token value appears anywhere in the TypeScript types, Zod schemas,
  or console.log output
```

---

### Supervisor Verification — Sprint S3

**V1 — Session types are correct.**
Open `shared/ipc-types.ts`. Confirm the `SessionInfo` interface has exactly these fields:
`is_authenticated`, `is_locked`, `user_id`, `username`, `display_name`, `is_admin`,
`force_password_change`, `expires_at`, `last_activity_at`. If any field is missing, flag it.

**V2 — No token in types.**
Search the file `shared/ipc-types.ts` for any field named `token`, `session_token`,
`access_token`, `bearer`, or `jwt`. If any such field exists, flag it as a security
contract violation.

**V3 — Hook tests pass.**
Run `pnpm run test`. Look for 4 test cases in `use-session.test.ts`. All should show
`ok` / green. If any fail, report the test name and error.

**V4 — AUTH_CONTRACTS.md documents security invariants.**
Open `docs/AUTH_CONTRACTS.md`. The document must contain the word "opaque" describing
the login error behavior, and the words "user enumeration" in its security invariants
section. If these are absent, flag it — the security rationale must be documented.

---

*End of Phase 1 · Sub-phase 04 · File 01*

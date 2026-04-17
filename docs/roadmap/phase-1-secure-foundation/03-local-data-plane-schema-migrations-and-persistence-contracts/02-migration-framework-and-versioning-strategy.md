# Phase 1 · Sub-phase 03 · File 02
# Migration Framework and Versioning Strategy

## Context and Purpose

File 01 created six foundational migrations. This file governs how ALL future migrations
in this project are written, versioned, reviewed, and protected. It is a rules-and-tooling
file, not a data-model file.

The concerns are:
1. **Naming and ordering** — a stable timestamp-based naming convention that prevents
   collision and makes migration history readable without a tool.
2. **Migration safety** — pre-migration WAL flush, the pre-destructive-action checkpoint
   pattern, and the startup refusal rule (§11.4) that prevents running a degraded app
   when migrations fail.
3. **Forward-only policy** — production migrations are append-only. The `down()` method
   exists for local development resets only and must never be run on production data.
4. **Rollback governance** — what to do when a migration fails in production: not a
   code rollback, but a recovery path.
5. **CI enforcement** — a CI job that prevents developers from introducing migrations
   that violate the naming scheme, that modify existing migrations, or that skip numbers.

## Prerequisites

- File 01 complete: migrations 001–006 all in place, cargo test green
- SP01-F03: `scripts/dev-db-reset.ts` in place for developer workflow
- SP02-F02: `startup.rs` with pre-migration checkpoint pattern (from Sub-phase 02 sprint plan)

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Migration Naming Rules and Pre-Migration Checkpoint | Naming doc, pre-migrate safety checkpoint in startup.rs, migration integrity test |
| S2 | Development Database Workflow and down() Policy | dev-db-reset update, migration test harness, forward-only policy enforcement |
| S3 | CI Migration Integrity Gate | GitHub Actions migration lint job, migration registry document |

---

## Sprint S1 — Migration Naming Rules and Pre-Migration Checkpoint

### AI Agent Prompt

```
You are a senior Rust engineer working on Maintafox Desktop. Six migrations (001–006)
are complete. Your task is to establish the migration framework governance: naming rules,
a startup-time pre-migration database checkpoint, and a test that verifies migrations run
in correct order without gaps.

────────────────────────────────────────────────────────────────────
STEP 1 — Create docs/MIGRATION_GUIDE.md
────────────────────────────────────────────────────────────────────
```markdown
# Migration Guide

## Naming Convention

All `sea-orm-migration` files MUST follow this naming scheme:

```
m<YYYYMMDD>_<NNNNNN>_<descriptive_name>.rs
```

| Segment | Rule |
|---------|------|
| `m` | Literal letter m — sea-orm prefix requirement |
| `YYYYMMDD` | The calendar date the migration was authored (UTC, 8 digits) |
| `NNNNNN` | 6-digit zero-padded sequential number, global to the project |
| `descriptive_name` | snake_case, max 40 characters, describes the schema change |

**Examples:**
```
m20260331_000001_system_tables.rs
m20260331_000006_teams_and_skills.rs
m20260415_000007_di_request_tables.rs
m20260415_000008_work_order_tables.rs
```

## Rules

1. **Append only.** Migration files are never renamed, reordered, or modified once
   committed to any shared branch. This rule has no exceptions in production.
2. **Sequential numbers.** The 6-digit sequence must increment by 1 for each new
   migration. Gaps are not allowed. If two migrations are authored on the same day,
   both keep their session date but increment the sequence.
3. **No down() in production.** The `down()` method exists for local developer database
   resets only. It must never be executed against a production or staging database.
4. **One concern per migration.** A migration file should address one logical schema
   change. Bundling unrelated table creations into one migration file is only acceptable
   during initial bootstrapping (migrations 001–006).
5. **Pre-destructive checkpoint.** Any migration that drops a column, drops a table, or
   renames a column is classified as "destructive" and requires a pre-migration backup
   checkpoint. See the Destructive Migration Protocol section below.
6. **Idempotency.** All `CREATE TABLE` statements use `IF NOT EXISTS`. All `CREATE INDEX`
   statements use `IF NOT EXISTS`. This prevents errors on retry after partial failure.

## Destructive Migration Protocol

A destructive migration is any migration that:
- drops a table
- drops a column from an existing table
- renames a column
- changes a column type
- removes a NOT NULL constraint from a column that had data

Before a destructive migration is applied:
1. The startup sequence must emit a `pre_destructive_migration` event to the frontend.
2. A WAL checkpoint is forced: `PRAGMA wal_checkpoint(FULL);`
3. A database file copy is made to `{app_data}/backups/pre_migration_<timestamp>.db`
4. The migration proceeds only if the backup copy succeeded.
5. If the backup fails, startup enters error state and presents a recovery option.

## Migration Failure Protocol

If a migration fails during startup:
- The startup sequence emits an error `StartupEvent` with phase `"error"` and a message.
- The AppShell renders the error screen (implemented in Sub-phase 02).
- The user is shown a "Contact support" and "Open backup folder" option.
- Normal UI entry is refused until migrations succeed (per PRD §11.4).

## Local Developer Reset

Developers may use `pnpm run db:reset` to drop the local database and re-run all
migrations from scratch. This is implemented by `scripts/dev-db-reset.ts` (created in
Sub-phase 01). The `down()` method is used internally by this script only.

## Adding a New Migration

1. Determine the next sequence number by reviewing `migrations/mod.rs`.
2. Create the file: `src-tauri/migrations/m<date>_<seq>_<name>.rs`
3. Implement `MigrationName::name()` to return EXACTLY the filename stem.
4. Add `up()` with `IF NOT EXISTS` guards.
5. Add `down()` for developer reset (drops in reverse order).
6. Register in `migrations/mod.rs` by adding `mod` declaration and adding to the
   `Migrator::migrations()` vec in the correct position (always at the end).
7. Run `cargo test` to verify the migration compiles.
8. Run `pnpm run db:reset && pnpm run dev` to verify the migration applies cleanly.
```

────────────────────────────────────────────────────────────────────
STEP 2 — Add pre-migration checkpoint to src-tauri/src/startup.rs
────────────────────────────────────────────────────────────────────
Before the migration runner is called in the startup sequence, add a WAL checkpoint
and a backup-before-destructive check. Add this function to startup.rs:

```rust
use std::path::PathBuf;

/// Forces a WAL checkpoint on the local SQLite database.
/// Must be called before any destructive migration is applied.
/// Returns Ok(()) on success or an error if the checkpoint fails.
pub async fn force_wal_checkpoint(db: &sea_orm::DatabaseConnection) -> crate::errors::AppResult<()> {
    use sea_orm::{ConnectionTrait, Statement, DbBackend};
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "PRAGMA wal_checkpoint(FULL);".to_string(),
    ))
    .await
    .map(|_| ())
    .map_err(|e| crate::errors::AppError::Database(
        format!("WAL checkpoint failed: {}", e)
    ))
}

/// Creates a pre-migration backup of the database file.
/// Called when detecting that a pending migration is classified as destructive.
pub fn backup_database(
    db_path: &PathBuf,
    backup_dir: &PathBuf,
) -> crate::errors::AppResult<PathBuf> {
    use std::time::SystemTime;
    use chrono::{DateTime, Utc};

    let timestamp = DateTime::<Utc>::from(SystemTime::now())
        .format("%Y%m%d_%H%M%S")
        .to_string();

    let backup_filename = format!("pre_migration_{}.db", timestamp);
    let backup_path = backup_dir.join(&backup_filename);

    std::fs::create_dir_all(backup_dir)
        .map_err(|e| crate::errors::AppError::Io(e.to_string()))?;

    std::fs::copy(db_path, &backup_path)
        .map_err(|e| crate::errors::AppError::Io(
            format!("Pre-migration backup failed: {}", e)
        ))?;

    tracing::info!(
        backup_path = %backup_path.display(),
        "startup::pre_migration_backup_complete"
    );

    Ok(backup_path)
}
```

Update AppError in errors.rs to include the `Io` variant if not already present:
```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // ... existing variants ...
    #[error("I/O error: {0}")]
    Io(String),
}
```

In the Serialize impl for AppError, add:
```rust
AppError::Io(_) => ("IO_ERROR", "Une erreur d'entrée/sortie s'est produite."),
```

────────────────────────────────────────────────────────────────────
STEP 3 — Add migration order integrity test to src-tauri/src/
────────────────────────────────────────────────────────────────────
Add a test file `src-tauri/src/db/migration_integrity.rs` that verifies the migration
list is correctly ordered and uses the naming convention:

```rust
// src-tauri/src/db/migration_integrity.rs

#[cfg(test)]
mod tests {
    /// Verifies migration names follow m<YYYYMMDD>_<NNNNNN>_<name> convention
    /// and that the sequence numbers are contiguous starting from 000001.
    #[test]
    fn migration_names_follow_convention() {
        let names = migration_name_list();
        for name in &names {
            assert!(
                name.starts_with('m'),
                "Migration name must start with 'm': {}",
                name
            );
            let parts: Vec<&str> = name.splitn(3, '_').collect();
            assert_eq!(
                parts.len(),
                3,
                "Migration name must have 3 underscore-separated segments: {}",
                name
            );
            let date_part = parts[0].trim_start_matches('m');
            assert_eq!(
                date_part.len(),
                8,
                "Date segment must be 8 digits (YYYYMMDD): {}",
                name
            );
            assert!(
                date_part.chars().all(|c| c.is_ascii_digit()),
                "Date segment must be all digits: {}",
                name
            );
            let seq_part = parts[1];
            assert_eq!(
                seq_part.len(),
                6,
                "Sequence segment must be 6 digits: {}",
                name
            );
            assert!(
                seq_part.chars().all(|c| c.is_ascii_digit()),
                "Sequence segment must be all digits: {}",
                name
            );
        }
    }

    #[test]
    fn migration_sequence_numbers_are_contiguous() {
        let names = migration_name_list();
        let mut seqs: Vec<u32> = names
            .iter()
            .map(|name| {
                let parts: Vec<&str> = name.splitn(3, '_').collect();
                parts[1].parse::<u32>().expect("Sequence must be numeric")
            })
            .collect();
        seqs.sort();

        for (i, seq) in seqs.iter().enumerate() {
            assert_eq!(
                *seq,
                (i + 1) as u32,
                "Migration sequence must be contiguous starting at 1. \
                 Expected {} at position {}, found {}",
                i + 1,
                i,
                seq
            );
        }
    }

    /// Returns the list of all registered migration name strings.
    /// Must be kept in sync with migrations/mod.rs.
    fn migration_name_list() -> Vec<String> {
        // Hardcoded list that mirrors the Migrator list.
        // When adding a new migration, add its name() return value here.
        vec![
            "m20260331_000001_system_tables".into(),
            "m20260331_000002_user_tables".into(),
            "m20260331_000003_reference_domains".into(),
            "m20260331_000004_org_schema".into(),
            "m20260331_000005_equipment_schema".into(),
            "m20260331_000006_teams_and_skills".into(),
        ]
    }
}
```

Register this module in `src-tauri/src/db/mod.rs`:
```rust
pub mod migration_integrity;
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures, including migration_integrity tests
- docs/MIGRATION_GUIDE.md is present and has all 5 sections
- force_wal_checkpoint and backup_database functions are present in startup.rs
- AppError::Io variant is present in errors.rs
- migration_integrity tests pass: names follow convention, sequence is 1–6
```

---

### Supervisor Verification — Sprint S1

**V1 — Migration integrity tests are green.**
Run:
```
cd src-tauri
cargo test db::migration_integrity
```
You should see `2 tests passed`. If any test fails, it means either a migration name
is formatted incorrectly or the sequence numbers have a gap. Flag the failing test name.

**V2 — Migration guide document is present.**
Open `docs/MIGRATION_GUIDE.md`. It should have sections for convention, rules, the
destructive protocol, and how to add a new migration. If the file is below 50 lines or
missing these topics, flag it.

**V3 — Rust tests still all pass.**
Run `cargo test` (no filter). All tests from previous files must still pass alongside
the new migration integrity tests. If any previously passing test now fails, flag it.

---

## Sprint S2 — Development Database Workflow and down() Policy

### AI Agent Prompt

```
You are a senior Rust and TypeScript engineer continuing work on Maintafox Desktop.
Sprint S1 is complete: migration naming conventions, documentation, and integrity tests
are all in place.

YOUR TASK: Formalize the development database workflow tools in scripts/, update the
db:reset and db:seed scripts to respect the new migration framework, and add a Cargo
integration test that runs all 6 migrations against an in-memory SQLite database to
confirm the complete schema applies cleanly.

────────────────────────────────────────────────────────────────────
STEP 1 — Update scripts/dev-db-reset.ts
────────────────────────────────────────────────────────────────────
The existing dev-db-reset.ts from Sub-phase 01 deletes the database file and lets
the app re-create it on next launch. Enhance it to also:
- Print which database file it is deleting (with an absolute path)
- Confirm the user wants to reset (read a y/n from stdin in CI-safe mode)
- Delete any existing backups older than 30 days from the backups directory
- Print a success message with next-step instructions

```typescript
// scripts/dev-db-reset.ts
import * as fs from "fs";
import * as path from "path";
import * as readline from "readline";
import * as os from "os";
import * as process from "process";

// Resolve the database path from .env or default
function getDbPath(): string {
  const envFile = path.join(process.cwd(), ".env");
  if (fs.existsSync(envFile)) {
    const content = fs.readFileSync(envFile, "utf8");
    const match = content.match(/^DATABASE_URL\s*=\s*(.+)$/m);
    if (match) {
      return match[1].trim().replace("sqlite://", "").replace(/\?.*/, "");
    }
  }
  // Tauri default: %APPDATA%/maintafox/maintafox.db on Windows
  const appData = process.env.APPDATA ?? path.join(os.homedir(), ".local", "share");
  return path.join(appData, "maintafox", "maintafox.db");
}

function getBackupsDir(dbPath: string): string {
  return path.join(path.dirname(dbPath), "backups");
}

async function confirm(message: string): Promise<boolean> {
  // In CI, auto-confirm
  if (process.env.CI === "true" || process.argv.includes("--yes")) {
    return true;
  }
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  return new Promise((resolve) => {
    rl.question(`${message} [y/N]: `, (answer) => {
      rl.close();
      resolve(answer.trim().toLowerCase() === "y");
    });
  });
}

function cleanOldBackups(backupsDir: string, maxAgeDays: number = 30): void {
  if (!fs.existsSync(backupsDir)) return;
  const now = Date.now();
  const maxAgeMs = maxAgeDays * 24 * 60 * 60 * 1000;
  const entries = fs.readdirSync(backupsDir);
  let deleted = 0;
  for (const entry of entries) {
    if (!entry.startsWith("pre_migration_")) continue;
    const fullPath = path.join(backupsDir, entry);
    const stat = fs.statSync(fullPath);
    if (now - stat.mtimeMs > maxAgeMs) {
      fs.unlinkSync(fullPath);
      deleted++;
    }
  }
  if (deleted > 0) {
    console.log(`Cleaned ${deleted} backup(s) older than ${maxAgeDays} days.`);
  }
}

async function main(): Promise<void> {
  const dbPath = getDbPath();
  const backupsDir = getBackupsDir(dbPath);

  console.log("=== Maintafox Dev DB Reset ===");
  console.log(`Database: ${dbPath}`);

  if (!fs.existsSync(dbPath)) {
    console.log("Database file does not exist. Nothing to delete.");
    console.log("Run 'pnpm run dev' to create and migrate the database.");
    process.exit(0);
  }

  const ok = await confirm(
    "This will DELETE the local database and all its data. Continue?"
  );
  if (!ok) {
    console.log("Reset cancelled.");
    process.exit(0);
  }

  // Also delete WAL and SHM companion files
  for (const ext of ["", "-wal", "-shm"]) {
    const p = `${dbPath}${ext}`;
    if (fs.existsSync(p)) {
      fs.unlinkSync(p);
      console.log(`Deleted: ${p}`);
    }
  }

  cleanOldBackups(backupsDir);

  console.log("\nDatabase reset complete.");
  console.log("Run 'pnpm run dev' to recreate and run all migrations.");
  console.log("Run 'pnpm run db:seed' after startup to restore development seed data.");
}

main().catch((e) => {
  console.error("Reset failed:", e);
  process.exit(1);
});
```

Update package.json scripts:
```json
"db:reset": "tsx scripts/dev-db-reset.ts",
"db:seed": "tsx scripts/dev-db-seed.ts",
"db:reset:ci": "tsx scripts/dev-db-reset.ts --yes"
```

────────────────────────────────────────────────────────────────────
STEP 2 — Add full-schema Cargo integration test
────────────────────────────────────────────────────────────────────
Create src-tauri/src/db/schema_integration_test.rs — a test that opens an in-memory
SQLite database and runs all 6 migrations, then confirms each expected table exists:

```rust
// src-tauri/src/db/schema_integration_test.rs
#[cfg(test)]
mod tests {
    /// Tests that all 6 Phase 1 migrations apply cleanly against a
    /// freshly created in-memory SQLite database.
    ///
    /// This test is the primary fast-feedback guard against:
    /// - Migration SQL syntax errors
    /// - Wrong column type declarations
    /// - Duplicate column names
    /// - Missing IF NOT EXISTS guards
    #[tokio::test]
    async fn all_migrations_apply_to_clean_database() {
        // Open in-memory SQLite for this test
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("Failed to open in-memory SQLite");

        // Enable foreign keys for the test
        use sea_orm::{ConnectionTrait, Statement, DbBackend};
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("Failed to enable foreign keys");

        // Run all migrations via the Migrator
        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migration failed — check migration SQL for syntax errors");

        // Verify expected tables exist by running COUNT queries
        let expected_tables = [
            "system_config",
            "trusted_devices",
            "audit_events",
            "app_sessions",
            "user_accounts",
            "roles",
            "permissions",
            "role_permissions",
            "user_scope_assignments",
            "lookup_domains",
            "lookup_values",
            "lookup_value_aliases",
            "org_structure_models",
            "org_node_types",
            "org_type_relationship_rules",
            "org_nodes",
            "org_node_responsibilities",
            "org_entity_bindings",
            "equipment_classes",
            "equipment",
            "equipment_hierarchy",
            "equipment_meters",
            "equipment_lifecycle_events",
            "skill_categories",
            "skill_definitions",
            "teams",
            "team_skill_requirements",
        ];

        for table in &expected_tables {
            let sql = format!("SELECT COUNT(*) FROM {};", table);
            db.execute(Statement::from_string(DbBackend::Sqlite, sql))
                .await
                .unwrap_or_else(|e| {
                    panic!("Table '{}' is missing or inaccessible: {}", table, e)
                });
        }
    }

    /// Verify critical column presence on the equipment table per §7.1 principles
    #[tokio::test]
    async fn equipment_table_has_required_sync_columns() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations");

        use sea_orm::{ConnectionTrait, Statement, DbBackend};
        // PRAGMA table_info returns one row per column; we check for our required columns
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(equipment);".to_string(),
            ))
            .await
            .expect("PRAGMA table_info failed");

        let columns: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap_or_default())
            .collect();

        for required in &[
            "id",
            "sync_id",
            "asset_id_code",
            "created_at",
            "updated_at",
            "deleted_at",
            "row_version",
            "origin_machine_id",
            "last_synced_checkpoint",
        ] {
            assert!(
                columns.contains(&required.to_string()),
                "equipment table is missing required column: {}",
                required
            );
        }
    }

    /// Verify equipment_lifecycle_events does NOT have deleted_at (append-only rule)
    #[tokio::test]
    async fn lifecycle_events_table_is_append_only() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations");

        use sea_orm::{ConnectionTrait, Statement, DbBackend};
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(equipment_lifecycle_events);".to_string(),
            ))
            .await
            .expect("PRAGMA");

        let columns: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").unwrap_or_default())
            .collect();

        assert!(
            !columns.contains(&"deleted_at".to_string()),
            "equipment_lifecycle_events must NOT have deleted_at — it is append-only"
        );

        assert!(
            !columns.contains(&"updated_at".to_string()),
            "equipment_lifecycle_events must NOT have updated_at — it is append-only"
        );
    }
}
```

Register this in db/mod.rs:
```rust
pub mod schema_integration_test;
```

Note: These tests require the `sea-orm` `sqlite` feature and `tokio` as dev-dependency,
both already present from SP01.

────────────────────────────────────────────────────────────────────
STEP 3 — Add the integration tests to the CI pipeline
────────────────────────────────────────────────────────────────────
In .github/workflows/ci.yml, in the `rust-quality` job, after the existing unit tests:

```yaml
      - name: Rust schema integration tests
        run: |
          cargo test --manifest-path src-tauri/Cargo.toml \
            db::schema_integration_test -- --nocapture
        env:
          SQLX_OFFLINE: "true"
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- cargo test passes with 0 failures (all 3 new integration tests pass)
- scripts/dev-db-reset.ts prompts for confirmation in interactive mode
- pnpm run db:reset:ci deletes the database without prompting
- CI yaml has the schema integration test step in rust-quality
```

---

### Supervisor Verification — Sprint S2

**V1 — Schema integration tests pass.**
Run:
```
cd src-tauri
cargo test db::schema_integration_test -- --nocapture
```
You should see `3 tests passed`. If any test fails, the error message will name the
missing table or column. Flag the exact message.

**V2 — all_migrations_apply_to_clean_database test covers all 27 tables.**
The test internally checks all 27 expected tables. If a migration has a typo in a table
name, this test will catch it. Confirm the test passed (green).

**V3 — db:reset script works interactively.**
From the terminal (not CI):
```
pnpm run db:reset
```
It should print the database path and ask `Continue? [y/N]:`. Type `n` and press Enter.
It should print `Reset cancelled.` and exit without deleting anything. If it deletes
the database without asking, flag it as a safety violation.

---

## Sprint S3 — CI Migration Integrity Gate

### AI Agent Prompt

```
You are a senior TypeScript and DevOps engineer working on Maintafox Desktop.
Sprints S1 and S2 are complete: migration naming conventions, documentation, dev tools,
and integration tests are all in place.

YOUR TASK: Create a CI job that enforces migration file integrity rules to prevent
common mistakes:
1. A new migration file was added without registering it in mod.rs
2. An existing migration file was modified (detected by hash change)
3. Migration sequence numbers have a gap
4. Integration tests now run in the full CI pipeline

────────────────────────────────────────────────────────────────────
STEP 1 — Create scripts/check-migrations.ts
────────────────────────────────────────────────────────────────────
```typescript
// scripts/check-migrations.ts
/**
 * Migration integrity checker.
 * Run by CI to verify migration files are correctly formed and registered.
 * Exit code 0 = all checks pass.
 * Exit code 1 = one or more checks failed.
 */
import * as fs from "fs";
import * as path from "path";
import * as process from "process";

const MIGRATIONS_DIR = path.join(
  process.cwd(),
  "src-tauri",
  "migrations",
);
const MOD_RS = path.join(MIGRATIONS_DIR, "mod.rs");

interface CheckResult {
  check: string;
  passed: boolean;
  message: string;
}

function getFilesystemMigrations(): string[] {
  if (!fs.existsSync(MIGRATIONS_DIR)) {
    throw new Error(`Migrations directory not found: ${MIGRATIONS_DIR}`);
  }
  return fs
    .readdirSync(MIGRATIONS_DIR)
    .filter((f) => f.startsWith("m2") && f.endsWith(".rs") && f !== "mod.rs")
    .map((f) => f.replace(".rs", ""))
    .sort();
}

function getRegisteredMigrations(): string[] {
  if (!fs.existsSync(MOD_RS)) {
    throw new Error(`migrations/mod.rs not found: ${MOD_RS}`);
  }
  const content = fs.readFileSync(MOD_RS, "utf8");
  const matches = content.match(/mod\s+(m\d{8}_\d{6}_\w+);/g) ?? [];
  return matches
    .map((m) => m.replace(/^mod\s+/, "").replace(/;$/, ""))
    .sort();
}

function extractSeqNumber(name: string): number {
  const parts = name.split("_");
  return parseInt(parts[1], 10);
}

function checkNamingConvention(names: string[]): CheckResult {
  const bad = names.filter((n) => {
    const parts = n.split("_");
    if (parts.length < 3) return true;
    const date = parts[0].slice(1); // remove leading 'm'
    const seq = parts[1];
    return (
      !/^\d{8}$/.test(date) ||
      !/^\d{6}$/.test(seq)
    );
  });
  return {
    check: "Naming convention",
    passed: bad.length === 0,
    message:
      bad.length === 0
        ? "All migration names follow the mYYYYMMDD_NNNNNN_name convention"
        : `Bad naming: ${bad.join(", ")}`,
  };
}

function checkContiguous(names: string[]): CheckResult {
  const seqs = names.map(extractSeqNumber).sort((a, b) => a - b);
  const gaps: string[] = [];
  for (let i = 0; i < seqs.length; i++) {
    if (seqs[i] !== i + 1) {
      gaps.push(`expected ${i + 1} but got ${seqs[i]}`);
    }
  }
  return {
    check: "Contiguous sequence",
    passed: gaps.length === 0,
    message:
      gaps.length === 0
        ? `Sequence is contiguous (1..${seqs.length})`
        : `Sequence gaps: ${gaps.join("; ")}`,
  };
}

function checkRegistration(
  filesystem: string[],
  registered: string[],
): CheckResult {
  const onDisk = new Set(filesystem);
  const inCode = new Set(registered);
  const unregistered = [...onDisk].filter((f) => !inCode.has(f));
  const phantom = [...inCode].filter((f) => !onDisk.has(f));
  const issues: string[] = [];
  if (unregistered.length > 0)
    issues.push(`On disk but not in mod.rs: ${unregistered.join(", ")}`);
  if (phantom.length > 0)
    issues.push(`In mod.rs but file missing: ${phantom.join(", ")}`);
  return {
    check: "Registration sync",
    passed: issues.length === 0,
    message:
      issues.length === 0
        ? "All migration files are registered in mod.rs"
        : issues.join("; "),
  };
}

function main(): void {
  console.log("=== Migration Integrity Check ===\n");

  const filesystem = getFilesystemMigrations();
  const registered = getRegisteredMigrations();

  console.log(`Found ${filesystem.length} migration file(s) on disk.`);
  console.log(`Found ${registered.length} migration(s) in mod.rs.\n`);

  const results: CheckResult[] = [
    checkNamingConvention(filesystem),
    checkContiguous(filesystem),
    checkRegistration(filesystem, registered),
  ];

  let allPassed = true;
  for (const r of results) {
    const icon = r.passed ? "✓" : "✗";
    console.log(`${icon} [${r.check}] ${r.message}`);
    if (!r.passed) allPassed = false;
  }

  console.log("\n" + (allPassed ? "All checks passed." : "CHECKS FAILED."));
  process.exit(allPassed ? 0 : 1);
}

main();
```

Add to package.json scripts:
```json
"migrate:check": "tsx scripts/check-migrations.ts"
```

────────────────────────────────────────────────────────────────────
STEP 2 — Add migrate:check to CI in .github/workflows/ci.yml
────────────────────────────────────────────────────────────────────
In the `lint-and-format` job (which runs pnpm-based checks), add after the lint step:

```yaml
      - name: Migration integrity check
        run: pnpm run migrate:check
```

This runs before the Rust steps and will fail fast if a developer accidentally added
a migration file without registering it in mod.rs.

────────────────────────────────────────────────────────────────────
STEP 3 — Create docs/MIGRATION_REGISTRY.md
────────────────────────────────────────────────────────────────────
This is the living registry of all migrations. It is updated every time a migration
is added in a Phase 2+ sprint. The AI agent prompt for each Phase 2 sprint that adds
a migration must update this file.

```markdown
# Migration Registry

Auto-updated as migrations are added. Never manually reorder this list.
This document is the human-readable companion to migrations/mod.rs.

## Format

| # | File Stem | Tables Created / Modified | Phase | Sub-phase |
|---|-----------|--------------------------|-------|-----------|
| 001 | m20260331_000001_system_tables | system_config, trusted_devices, audit_events, app_sessions | 1 | 01 |
| 002 | m20260331_000002_user_tables | user_accounts, roles, permissions, role_permissions, user_scope_assignments | 1 | 01 |
| 003 | m20260331_000003_reference_domains | lookup_domains, lookup_values, lookup_value_aliases | 1 | 03 |
| 004 | m20260331_000004_org_schema | org_structure_models, org_node_types, org_type_relationship_rules, org_nodes, org_node_responsibilities, org_entity_bindings | 1 | 03 |
| 005 | m20260331_000005_equipment_schema | equipment_classes, equipment, equipment_hierarchy, equipment_meters, equipment_lifecycle_events | 1 | 03 |
| 006 | m20260331_000006_teams_and_skills | skill_categories, skill_definitions, teams, team_skill_requirements | 1 | 03 |

## Upcoming (reserved for Phase 2)

| Planned # | Working Title | Planned Sub-phase |
|-----------|---------------|-------------------|
| 007 | personnel_tables | Phase 2 · SP01 Personnel |
| 008 | di_request_tables | Phase 2 · SP02 DI |
| 009 | work_order_tables | Phase 2 · SP03 Work Orders |
| 010 | inventory_tables | Phase 2 · SP04 Inventory |
| 011 | pm_tables | Phase 2 · SP05 PM |
| 012 | notification_tables | Phase 2 · SP07 Notifications |
| 013 | planning_tables | Phase 2 · SP08 Planning |
| 014 | activity_audit_tables | Phase 2 · SP09 Activity/Audit |

## Destructive Migration History

No destructive migrations have been applied yet. This section records every migration
that included a DROP or RENAME statement plus the pre-destructive backup path.

*(empty)*
```

────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
────────────────────────────────────────────────────────────────────
- pnpm run migrate:check exits with code 0 (all 3 checks pass)
- Adding a test migration file to src-tauri/migrations/ WITHOUT registering it in mod.rs
  causes pnpm run migrate:check to exit with code 1 (test and revert this manually)
- docs/MIGRATION_REGISTRY.md is created with all 6 migrations listed
- CI yaml has the migration integrity check step in lint-and-format job
```

---

### Supervisor Verification — Sprint S3

**V1 — Check script passes.**
Run:
```
pnpm run migrate:check
```
All three checks should show a green tick (✓): Naming convention, Contiguous sequence,
Registration sync. If any shows ✗, flag it.

**V2 — Check script catches a missing registration.**
Ask the agent to temporarily rename one migration file to `test_extra_migration.rs`
(or create a dummy file), run `pnpm run migrate:check`, and confirm it exits with code 1
and prints a relevant error. Then undo the change. If the script does not catch this,
flag it as a safety gap.

**V3 — Migration registry document is present.**
Open `docs/MIGRATION_REGISTRY.md`. It should show a table with 6 rows (001–006) and the
"Upcoming" section listing planned Phase 2 migrations. If the file is absent or has fewer
than 6 table rows, flag it.

**V4 — CI yaml includes the migration check step.**
Open `.github/workflows/ci.yml`. In the `lint-and-format` job, verify there is a step
named `Migration integrity check` that runs `pnpm run migrate:check`. If this step is
absent, the CI cannot catch unregistered migrations. Flag it if it is absent.

---

*End of Phase 1 · Sub-phase 03 · File 02*
*Next: File 03 — Repository Services and Persistence Contracts*

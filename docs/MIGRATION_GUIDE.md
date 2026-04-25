# Migration Guide

## Naming Convention

All `sea-orm-migration` files MUST follow this naming scheme:

```
m<YYYYMMDD>_<NNNNNN>_<descriptive_name>.rs
```

| Segment      | Rule                                                        |
|--------------|-------------------------------------------------------------|
| `m`          | Literal letter m — sea-orm prefix requirement               |
| `YYYYMMDD`   | The calendar date the migration was authored (UTC, 8 digits)|
| `NNNNNN`     | 6-digit zero-padded sequential number, global to the project|
| `descriptive_name` | snake_case, max 40 characters, describes the schema change |

**Examples:**
```
m20260401_000001_system_tables.rs
m20260401_000006_teams_and_skills.rs
m20260415_000007_di_request_tables.rs
m20260415_000008_work_order_tables.rs
```

## Rules

1. **Append only.** Migration files are never renamed, reordered, or modified once committed to any shared branch. This rule has no exceptions in production.
2. **Sequential numbers.** The 6-digit sequence must increment by 1 for each new migration. Gaps are not allowed. If two migrations are authored on the same day, both keep their session date but increment the sequence.
3. **No down() in production.** The `down()` method exists for local developer database resets only. It must never be executed against a production or staging database.
4. **One concern per migration.** A migration file should address one logical schema change. Bundling unrelated table creations into one migration file is only acceptable during initial bootstrapping (migrations 001–006).
5. **Pre-destructive checkpoint.** Any migration that drops a column, drops a table, or renames a column is classified as "destructive" and requires a pre-migration backup checkpoint. See the Destructive Migration Protocol section below.
6. **Idempotency.** All `CREATE TABLE` statements use `IF NOT EXISTS`. All `CREATE INDEX` statements use `IF NOT EXISTS`. This prevents errors on retry after partial failure.

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

Developers may use `pnpm run db:reset` to drop the local database and re-run all migrations from scratch. This is implemented by `scripts/dev-db-reset.ts` (created in Sub-phase 01). The `down()` method is used internally by this script only.

## Adding a New Migration

1. Determine the next sequence number by reviewing `migrations/mod.rs`.
2. Create the file: `src-tauri/migrations/m<date>_<seq>_<name>.rs`
3. Implement `MigrationName::name()` to return EXACTLY the filename stem.
4. Add `up()` with `IF NOT EXISTS` guards.
5. Add `down()` for developer reset (drops in reverse order).
6. Register in `migrations/mod.rs` by adding `mod` declaration and adding to the `Migrator::migrations()` vec in the correct position (always at the end).
7. Run `cargo test` to verify the migration compiles.
8. Run `pnpm run db:reset && pnpm run dev` to verify the migration applies cleanly.

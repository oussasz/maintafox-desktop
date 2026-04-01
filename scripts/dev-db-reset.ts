import Database from "better-sqlite3";
import { execSync } from "child_process";
import { rmSync, existsSync, mkdirSync } from "fs";
import { resolve } from "path";

const dbPath = resolve(process.cwd(), "dev-data", "maintafox_dev.db");
const dbDir = resolve(process.cwd(), "dev-data");

console.log("Resetting development database...");

// Remove existing DB and WAL/SHM files
for (const suffix of ["", "-wal", "-shm"]) {
  const file = dbPath + suffix;
  if (existsSync(file)) {
    rmSync(file);
  }
}
if (existsSync(dbPath)) {
  console.log("  Removed existing database file.");
}

if (!existsSync(dbDir)) {
  mkdirSync(dbDir, { recursive: true });
  console.log("  Created dev-data/ directory.");
}

// Create DB and apply migrations (matches sea-orm migration definitions)
console.log("  Applying migrations...");
const db = new Database(dbPath);

// SQLite pragmas
db.pragma("journal_mode = WAL");
db.pragma("foreign_keys = ON");
db.pragma("busy_timeout = 5000");
db.pragma("cache_size = -20000");
db.pragma("temp_store = MEMORY");

// Migration 1: system tables
db.exec(`
  CREATE TABLE IF NOT EXISTS system_config (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    value TEXT,
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS trusted_devices (
    id TEXT NOT NULL PRIMARY KEY,
    device_fingerprint TEXT NOT NULL UNIQUE,
    device_label TEXT,
    user_id TEXT NOT NULL,
    trusted_at TEXT NOT NULL,
    last_seen_at TEXT,
    is_revoked INTEGER NOT NULL DEFAULT 0,
    revoked_at TEXT,
    revoked_reason TEXT
  );

  CREATE TABLE IF NOT EXISTS audit_events (
    id TEXT NOT NULL PRIMARY KEY,
    event_type TEXT NOT NULL,
    actor_id TEXT,
    actor_name TEXT,
    entity_type TEXT,
    entity_id TEXT,
    summary TEXT,
    detail_json TEXT,
    device_id TEXT,
    occurred_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS app_sessions (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    device_id TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_activity_at TEXT,
    is_revoked INTEGER NOT NULL DEFAULT 0
  );
`);

// Migration 2: user tables
db.exec(`
  CREATE TABLE IF NOT EXISTS user_accounts (
    id TEXT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    identity_mode TEXT NOT NULL DEFAULT 'local',
    personnel_id TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    force_password_change INTEGER NOT NULL DEFAULT 1,
    last_seen_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS roles (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    is_system INTEGER NOT NULL DEFAULT 0,
    role_type TEXT NOT NULL DEFAULT 'custom',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS permissions (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    category TEXT,
    is_dangerous INTEGER NOT NULL DEFAULT 0,
    requires_step_up INTEGER NOT NULL DEFAULT 0
  );

  CREATE TABLE IF NOT EXISTS role_permissions (
    role_id TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    PRIMARY KEY (role_id, permission_id)
  );

  CREATE TABLE IF NOT EXISTS user_scope_assignments (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    role_id TEXT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_reference TEXT,
    valid_from TEXT,
    valid_to TEXT
  );

  -- sea-orm migration tracking table
  CREATE TABLE IF NOT EXISTS seaql_migrations (
    version TEXT NOT NULL PRIMARY KEY,
    applied_at INTEGER NOT NULL
  );
`);

// Record migrations as applied so sea-orm doesn't re-run them at app startup
const now = Math.floor(Date.now() / 1000);
const insertMigration = db.prepare(
  "INSERT OR IGNORE INTO seaql_migrations (version, applied_at) VALUES (?, ?)",
);
insertMigration.run("m20260401_000001_system_tables", now);
insertMigration.run("m20260401_000002_user_tables", now);

db.close();
console.log("  Migrations applied.");

// Seed baseline data
console.log("  Seeding baseline data...");
execSync("pnpm tsx scripts/dev-db-seed.ts", { stdio: "inherit" });

console.log("\nDatabase reset complete: dev-data/maintafox_dev.db");

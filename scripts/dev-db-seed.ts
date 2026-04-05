import Database from "better-sqlite3";
import { resolve } from "path";
import { randomUUID } from "crypto";

const DB_PATH = resolve(process.cwd(), "dev-data", "maintafox_dev.db");
const db = new Database(DB_PATH);

const now = new Date().toISOString();

// System config baseline
const insertConfig = db.prepare(
  "INSERT OR IGNORE INTO system_config (key, value, updated_at) VALUES (?, ?, ?)",
);
insertConfig.run("app_version", "0.1.0-dev", now);
insertConfig.run("locale_default", "fr", now);
insertConfig.run("offline_grace_days", "7", now);

// Default system roles (PRD Section 6.7)
const insertRole = db.prepare(
  `INSERT OR IGNORE INTO roles (id, name, description, is_system, role_type, status, created_at)
   VALUES (?, ?, ?, ?, ?, ?, ?)`,
);

const systemRoles = [
  ["Administrator", "Full access to all modules and settings", true],
  ["Planner", "Plan and schedule work orders; manage backlog", true],
  ["Technician", "Execute work orders; record actuals and close-out", true],
  ["Supervisor", "Approve requests; review and verify completed work", true],
  ["Storekeeper", "Manage spare parts, inventory movements, and procurement", true],
  ["Viewer", "Read-only access to work history and reports", true],
] as const;

for (const [name, desc, isSys] of systemRoles) {
  insertRole.run(randomUUID(), name, desc, isSys ? 1 : 0, "system", "active", now);
}

// Development admin account
const insertUser = db.prepare(
  `INSERT OR IGNORE INTO user_accounts
   (id, username, identity_mode, is_active, force_password_change, created_at, updated_at)
   VALUES (?, ?, ?, ?, ?, ?, ?)`,
);
insertUser.run(randomUUID(), "admin", "local", 1, 1, now, now);

db.close();
console.log("Seed data inserted successfully.");
console.log("  - system_config: 3 entries");
console.log("  - roles: 6 system roles");
console.log("  - user_accounts: admin (password change required on first login)");

// One-shot script: insert Administrator scope assignment for admin user
// Run with: node --experimental-sqlite scripts/seed-admin-scope.mjs

import { DatabaseSync } from "node:sqlite";

const DB_PATH =
  "C:\\Users\\LENOVO\\AppData\\Roaming\\systems.maintafox.desktop\\maintafox.db";

const db = new DatabaseSync(DB_PATH);

// Check if assignment already exists
const existing = db
  .prepare(
    `SELECT COUNT(*) as cnt
     FROM user_scope_assignments usa
     JOIN user_accounts ua ON ua.id = usa.user_id
     JOIN roles r ON r.id = usa.role_id
     WHERE ua.username = 'admin' AND r.name = 'Administrator' AND usa.scope_type = 'tenant'`
  )
  .get();

if (existing.cnt > 0) {
  console.log(`Scope assignment already exists (${existing.cnt} row(s)). Skipping insert.`);
} else {
  db.exec(
    `INSERT INTO user_scope_assignments
       (sync_id, user_id, role_id, scope_type, created_at, updated_at, row_version)
     SELECT hex(randomblob(8)), id,
            (SELECT id FROM roles WHERE name = 'Administrator'),
            'tenant', datetime('now'), datetime('now'), 1
     FROM user_accounts WHERE username = 'admin'`
  );
  console.log("Inserted Administrator tenant scope assignment for admin.");
}

// Verify
const rows = db
  .prepare(
    `SELECT usa.sync_id, ua.username, r.name as role, usa.scope_type, usa.created_at
     FROM user_scope_assignments usa
     JOIN user_accounts ua ON ua.id = usa.user_id
     JOIN roles r ON r.id = usa.role_id
     WHERE ua.username = 'admin'`
  )
  .all();

console.log("\nCurrent scope assignments for admin:");
console.table(rows);
db.close();

/**
 * Sub-phase 03 Completion Verification Script
 * Checks #5, #6, #8, #10, #11 from the checklist (DB-level checks)
 */
const path = require("path");
const Database = require("better-sqlite3");

const DB_PATH = path.join(
  process.env.APPDATA,
  "systems.maintafox.desktop",
  "maintafox.db"
);

const db = new Database(DB_PATH, { readonly: true });
let allPass = true;

function check(num, label, fn) {
  try {
    const result = fn();
    const status = result ? "PASS" : "FAIL";
    if (!result) allPass = false;
    console.log(`  [${status}] #${num} — ${label}`);
  } catch (e) {
    allPass = false;
    console.log(`  [FAIL] #${num} — ${label}: ${e.message}`);
  }
}

console.log("\n=== Sub-phase 03 Completion — DB Checks ===\n");

// #5: 18 domains, ≥90 values
check(5, "18 rows in lookup_domains", () => {
  const row = db.prepare("SELECT COUNT(*) as cnt FROM lookup_domains WHERE deleted_at IS NULL").get();
  console.log(`       → domain_count = ${row.cnt}`);
  return row.cnt === 18;
});

check("5b", "≥90 rows in lookup_values", () => {
  const row = db.prepare("SELECT COUNT(*) as cnt FROM lookup_values WHERE deleted_at IS NULL").get();
  console.log(`       → value_count = ${row.cnt}`);
  return row.cnt >= 90;
});

// #6: seed_schema_version = 1
check(6, "system_config has seed_schema_version = 1", () => {
  const row = db.prepare("SELECT value FROM system_config WHERE key = 'seed_schema_version'").get();
  console.log(`       → seed_schema_version = ${row?.value}`);
  return row && row.value === "1";
});

// #8: migration 004 tables
const migration004Tables = [
  "org_structure_models", "org_node_types", "org_type_relationship_rules",
  "org_nodes", "org_node_responsibilities", "org_entity_bindings"
];
check(8, "Migration 004 tables present (org_structure_models through org_entity_bindings)", () => {
  for (const t of migration004Tables) {
    const exists = db.prepare(`SELECT name FROM sqlite_master WHERE type='table' AND name=?`).get(t);
    if (!exists) { console.log(`       → MISSING: ${t}`); return false; }
  }
  console.log(`       → All ${migration004Tables.length} tables found`);
  return true;
});

// #9 (migration 005 tables): equipment_classes through equipment_lifecycle_events
const migration005Tables = [
  "equipment_classes", "equipment", "equipment_hierarchy",
  "equipment_meters", "equipment_lifecycle_events"
];
check(9, "Migration 005 tables present (equipment_classes through equipment_lifecycle_events)", () => {
  for (const t of migration005Tables) {
    const exists = db.prepare(`SELECT name FROM sqlite_master WHERE type='table' AND name=?`).get(t);
    if (!exists) { console.log(`       → MISSING: ${t}`); return false; }
  }
  console.log(`       → All ${migration005Tables.length} tables found`);
  return true;
});

// #10: equipment_lifecycle_events has NO deleted_at column
check(10, "equipment_lifecycle_events has NO deleted_at column", () => {
  const cols = db.prepare("PRAGMA table_info(equipment_lifecycle_events)").all();
  const hasDeletedAt = cols.some(c => c.name === "deleted_at");
  console.log(`       → deleted_at present: ${hasDeletedAt}`);
  return !hasDeletedAt;
});

// #11: migration 006 tables
const migration006Tables = [
  "skill_categories", "skill_definitions", "teams", "team_skill_requirements"
];
check(11, "Migration 006 tables present (skill_categories through team_skill_requirements)", () => {
  for (const t of migration006Tables) {
    const exists = db.prepare(`SELECT name FROM sqlite_master WHERE type='table' AND name=?`).get(t);
    if (!exists) { console.log(`       → MISSING: ${t}`); return false; }
  }
  console.log(`       → All 4 tables found`);
  return true;
});

db.close();
console.log(`\n=== Result: ${allPass ? "ALL PASSED" : "SOME FAILED"} ===\n`);
process.exit(allPass ? 0 : 1);

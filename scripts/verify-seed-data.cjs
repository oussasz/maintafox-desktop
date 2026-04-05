// Verification script for Sprint S1 — Seed Data Integrity
// Run: node scripts/verify-seed-data.js

const path = require("path");
const os = require("os");
const Database = require("better-sqlite3");

const dbPath = path.join(
  os.homedir(),
  "AppData",
  "Roaming",
  "systems.maintafox.desktop",
  "maintafox.db"
);

console.log("Database: " + dbPath);
console.log("=".repeat(60));

const db = new Database(dbPath, { readonly: true });

// V1: Domain and value counts
const domainCount = db.prepare("SELECT COUNT(*) as cnt FROM lookup_domains").get().cnt;
const valueCount = db.prepare("SELECT COUNT(*) as cnt FROM lookup_values").get().cnt;
console.log("\n=== V1 — Seed data is present after startup ===");
console.log("  lookup_domains: " + domainCount + " rows");
console.log("  lookup_values:  " + valueCount + " rows");
console.log("  Expected: 18 domains, >= 90 values");
console.log("  RESULT: " + (domainCount === 18 && valueCount >= 90 ? "PASS ✓" : "FAIL ✗"));

// V2: Seed schema version
const verRow = db.prepare("SELECT value FROM system_config WHERE key = 'seed_schema_version'").get();
const ver = verRow ? verRow.value : null;
console.log("\n=== V2 — Seed schema version is recorded ===");
console.log("  seed_schema_version: " + ver);
console.log("  Expected: 1");
console.log("  RESULT: " + (ver === "1" ? "PASS ✓" : "FAIL ✗"));

// V4: French labels in equipment.criticality
const rows = db.prepare(`
  SELECT lv.code, lv.label, lv.fr_label
  FROM lookup_values lv
  JOIN lookup_domains ld ON ld.id = lv.domain_id
  WHERE ld.domain_key = 'equipment.criticality'
  ORDER BY lv.sort_order
`).all();
console.log("\n=== V4 — French labels in equipment.criticality ===");
console.log("  Rows found: " + rows.length);
for (const r of rows) {
  console.log("  " + r.code.padEnd(15) + " label=" + (r.label || "").padEnd(15) + " fr_label=" + r.fr_label);
}
const expectedCodes = new Set(["CRITIQUE", "IMPORTANT", "STANDARD", "NON_CRITIQUE"]);
const actualCodes = new Set(rows.map((r) => r.code));
const match =
  expectedCodes.size === actualCodes.size &&
  [...expectedCodes].every((c) => actualCodes.has(c));
console.log("  Expected codes: CRITIQUE, IMPORTANT, STANDARD, NON_CRITIQUE");
console.log("  RESULT: " + (match && rows.length === 4 ? "PASS ✓" : "FAIL ✗"));

// Summary: all domains
console.log("\n=== All lookup domains ===");
const allDomains = db.prepare("SELECT domain_key, display_name, domain_type FROM lookup_domains ORDER BY id").all();
for (const d of allDomains) {
  const vCount = db.prepare("SELECT COUNT(*) as cnt FROM lookup_values WHERE domain_id = (SELECT id FROM lookup_domains WHERE domain_key = ?)").get(d.domain_key).cnt;
  console.log("  " + d.domain_key.padEnd(40) + " " + d.domain_type.padEnd(8) + " values=" + vCount);
}

console.log("\n" + "=".repeat(60));
const allPass = domainCount === 18 && valueCount >= 90 && ver === "1" && match && rows.length === 4;
console.log("OVERALL: " + (allPass ? "ALL VERIFICATIONS PASSED ✓" : "SOME VERIFICATIONS FAILED ✗"));

db.close();

#!/usr/bin/env tsx
/**
 * check-i18n-parity.ts
 *
 * Verifies that all translation JSON files in fr/ have matching key trees in en/
 * and vice versa. Skips:
 *   - formats.json (locale-specific tokens — intentionally different)
 *   - namespace pairs where BOTH files are empty objects {} (pending translation)
 *
 * Exit codes:
 *   0 — All checked namespaces are in parity
 *   1 — One or more namespaces have mismatched keys
 *
 * Usage:
 *   pnpm run i18n:check
 *   tsx scripts/check-i18n-parity.ts
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");

// ─── Configuration ──────────────────────────────────────────────────────────
// These lists must stay in sync with src/i18n/namespaces.ts.
// "formats" is intentionally excluded — it contains locale-specific tokens.

const EAGER_NAMESPACES = ["common", "auth", "errors", "validation", "shell"];

const MODULE_NAMESPACES = [
  "equipment",
  "di",
  "ot",
  "org",
  "personnel",
  "reference",
  "inventory",
  "pm",
  "planning",
  "permits",
  "inspections",
  "training",
  "reliability",
  "budget",
  "reports",
  "archive",
  "notifications",
  "documentation",
  "iot",
  "erp",
  "activity",
  "users",
  "settings",
  "diagnostics",
  "configuration",
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

type JsonObject = Record<string, unknown>;

function readJson(filePath: string): JsonObject | null {
  if (!fs.existsSync(filePath)) {
    console.error(`  ✗ File not found: ${path.relative(ROOT, filePath)}`);
    return null;
  }
  try {
    const raw = fs.readFileSync(filePath, "utf-8");
    return JSON.parse(raw) as JsonObject;
  } catch (err) {
    console.error(`  ✗ Invalid JSON: ${path.relative(ROOT, filePath)}`);
    console.error(`    ${String(err)}`);
    return null;
  }
}

function collectLeafKeys(obj: JsonObject, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      return collectLeafKeys(value as JsonObject, fullKey);
    }
    return [fullKey];
  });
}

function isEmpty(obj: JsonObject): boolean {
  return Object.keys(obj).length === 0;
}

// ─── Check logic ──────────────────────────────────────────────────────────────

interface ParityResult {
  namespace: string;
  skipped: boolean;
  skipReason?: string;
  missingInEn: string[];
  missingInFr: string[];
  parseError: boolean;
}

function checkParity(namespace: string, frPath: string, enPath: string): ParityResult {
  const frJson = readJson(frPath);
  const enJson = readJson(enPath);

  if (frJson === null || enJson === null) {
    return {
      namespace,
      skipped: false,
      missingInEn: [],
      missingInFr: [],
      parseError: true,
    };
  }

  if (isEmpty(frJson) && isEmpty(enJson)) {
    return {
      namespace,
      skipped: true,
      skipReason: "both files are empty placeholders {}",
      missingInEn: [],
      missingInFr: [],
      parseError: false,
    };
  }

  const frKeys = new Set(collectLeafKeys(frJson));
  const enKeys = new Set(collectLeafKeys(enJson));

  const missingInEn = [...frKeys].filter((k) => !enKeys.has(k)).sort();
  const missingInFr = [...enKeys].filter((k) => !frKeys.has(k)).sort();

  return {
    namespace,
    skipped: false,
    missingInEn,
    missingInFr,
    parseError: false,
  };
}

// ─── Main ─────────────────────────────────────────────────────────────────────

function main(): void {
  console.log("╔══════════════════════════════════════════════════╗");
  console.log("║   Maintafox — Translation Parity Check           ║");
  console.log("╚══════════════════════════════════════════════════╝");
  console.log("");

  const results: ParityResult[] = [];
  let hasErrors = false;

  // Check eager namespace files
  console.log("─── Eager namespaces ──────────────────────────────");
  for (const ns of EAGER_NAMESPACES) {
    const frPath = path.join(ROOT, "src", "i18n", "fr", `${ns}.json`);
    const enPath = path.join(ROOT, "src", "i18n", "en", `${ns}.json`);
    const result = checkParity(ns, frPath, enPath);
    results.push(result);

    if (result.parseError) {
      console.log(`  ✗ ${ns.padEnd(20)} parse error (see above)`);
      hasErrors = true;
    } else if (result.skipped) {
      console.log(`  ○ ${ns.padEnd(20)} skipped — ${result.skipReason}`);
    } else if (result.missingInEn.length === 0 && result.missingInFr.length === 0) {
      console.log(`  ✓ ${ns.padEnd(20)} OK`);
    } else {
      console.log(`  ✗ ${ns.padEnd(20)} MISMATCH`);
      hasErrors = true;
    }
  }

  // Check module namespace files
  console.log("");
  console.log("─── Module namespaces ─────────────────────────────");
  for (const ns of MODULE_NAMESPACES) {
    const frPath = path.join(ROOT, "src", "i18n", "locale-data", "fr", `${ns}.json`);
    const enPath = path.join(ROOT, "src", "i18n", "locale-data", "en", `${ns}.json`);
    const result = checkParity(ns, frPath, enPath);
    results.push(result);

    if (result.parseError) {
      console.log(`  ✗ ${ns.padEnd(20)} parse error (see above)`);
      hasErrors = true;
    } else if (result.skipped) {
      console.log(`  ○ ${ns.padEnd(20)} placeholder (both empty)`);
    } else if (result.missingInEn.length === 0 && result.missingInFr.length === 0) {
      console.log(`  ✓ ${ns.padEnd(20)} OK`);
    } else {
      console.log(`  ✗ ${ns.padEnd(20)} MISMATCH`);
      hasErrors = true;
    }
  }

  // Detailed error report
  const errored = results.filter(
    (r) => !r.skipped && !r.parseError && (r.missingInEn.length > 0 || r.missingInFr.length > 0),
  );

  if (errored.length > 0) {
    console.log("");
    console.log("─── Mismatch details ──────────────────────────────");
    for (const r of errored) {
      console.log(`\n  Namespace: ${r.namespace}`);
      if (r.missingInEn.length > 0) {
        console.log(`  Missing in en/ (${r.missingInEn.length} keys):`);
        for (const k of r.missingInEn) console.log(`    - ${k}`);
      }
      if (r.missingInFr.length > 0) {
        console.log(`  Missing in fr/ (${r.missingInFr.length} keys):`);
        for (const k of r.missingInFr) console.log(`    + ${k}`);
      }
    }
  }

  // Summary
  const checked = results.filter((r) => !r.skipped && !r.parseError);
  const passed = checked.filter((r) => r.missingInEn.length === 0 && r.missingInFr.length === 0);
  const skipped = results.filter((r) => r.skipped);
  const parseErrors = results.filter((r) => r.parseError);

  console.log("");
  console.log("─── Summary ───────────────────────────────────────");
  console.log(`  Checked:      ${checked.length}`);
  console.log(`  Passed:       ${passed.length}`);
  console.log(`  Mismatches:   ${checked.length - passed.length}`);
  console.log(`  Skipped:      ${skipped.length} (placeholder pairs)`);
  console.log(`  Parse errors: ${parseErrors.length}`);
  console.log("");

  if (hasErrors) {
    console.log("  ✗ Parity check FAILED. Fix mismatches before merging.");
    process.exit(1);
  } else {
    console.log("  ✓ All translation files are in parity.");
    process.exit(0);
  }
}

main();

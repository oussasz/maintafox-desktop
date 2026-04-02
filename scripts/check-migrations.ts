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

const MIGRATIONS_DIR = path.join(process.cwd(), "src-tauri", "src", "migrations");
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
  return matches.map((m) => m.replace(/^mod\s+/, "").replace(/;$/, "")).sort();
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
    return !/^\d{8}$/.test(date) || !/^\d{6}$/.test(seq);
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

function checkRegistration(filesystem: string[], registered: string[]): CheckResult {
  const onDisk = new Set(filesystem);
  const inCode = new Set(registered);
  const unregistered = [...onDisk].filter((f) => !inCode.has(f));
  const phantom = [...inCode].filter((f) => !onDisk.has(f));
  const issues: string[] = [];
  if (unregistered.length > 0) issues.push(`On disk but not in mod.rs: ${unregistered.join(", ")}`);
  if (phantom.length > 0) issues.push(`In mod.rs but file missing: ${phantom.join(", ")}`);
  return {
    check: "Registration sync",
    passed: issues.length === 0,
    message:
      issues.length === 0 ? "All migration files are registered in mod.rs" : issues.join("; "),
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
    const icon = r.passed ? "\u2713" : "\u2717";
    console.log(`${icon} [${r.check}] ${r.message}`);
    if (!r.passed) allPassed = false;
  }

  console.log("\n" + (allPassed ? "All checks passed." : "CHECKS FAILED."));
  process.exit(allPassed ? 0 : 1);
}

main();

// Maintafox Desktop — environment preflight checker
// Usage: pnpm tsx scripts/check-env.ts
// Prints a PASS/FAIL table and exits with code 1 if any check fails.

import { execSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

interface Check {
  name: string;
  pass: boolean;
  detail: string;
}

const checks: Check[] = [];

function check(name: string, fn: () => { pass: boolean; detail: string }): void {
  try {
    const result = fn();
    checks.push({ name, ...result });
  } catch (e) {
    checks.push({ name, pass: false, detail: String(e) });
  }
}

// ── 1. Node.js version ────────────────────────────────────────────────────────
check("Node.js >= 20", () => {
  const v = process.version; // e.g. "v20.11.0"
  const major = parseInt(v.slice(1).split(".")[0] ?? "0", 10);
  return {
    pass: major >= 20,
    detail: major >= 20 ? v : `Found ${v} — needs v20+`,
  };
});

// ── 2. pnpm version ───────────────────────────────────────────────────────────
check("pnpm >= 9", () => {
  const v = execSync("pnpm --version", { encoding: "utf-8" }).trim();
  const major = parseInt(v.split(".")[0] ?? "0", 10);
  return {
    pass: major >= 9,
    detail: major >= 9 ? `v${v}` : `Found v${v} — needs v9+`,
  };
});

// ── 3. MAINTAFOX_ENV is set ───────────────────────────────────────────────────
check("MAINTAFOX_ENV is set", () => {
  // Read from .env.local if it exists (tsx does not auto-load .env files)
  const envPath = resolve(process.cwd(), ".env.local");
  let envValue = process.env["MAINTAFOX_ENV"];
  if (!envValue && existsSync(envPath)) {
    const raw = readFileSync(envPath, "utf-8");
    const match = raw.match(/^MAINTAFOX_ENV=(.+)$/m);
    envValue = match?.[1]?.trim();
  }
  const valid = ["development", "staging", "production"];
  const pass = typeof envValue === "string" && valid.includes(envValue);
  return {
    pass,
    detail: pass ? envValue! : `Got "${envValue ?? "unset"}" — must be: ${valid.join(" | ")}`,
  };
});

// ── 4. .env.local exists ──────────────────────────────────────────────────────
check(".env.local exists", () => {
  const exists = existsSync(resolve(process.cwd(), ".env.local"));
  return {
    pass: exists,
    detail: exists ? "found" : "Missing — run setup script to create it",
  };
});

// ── 5. No signing key in dev environment ─────────────────────────────────────
check("No signing key in .env.local (security)", () => {
  const envPath = resolve(process.cwd(), ".env.local");
  if (!existsSync(envPath)) {
    return { pass: true, detail: "no .env.local to check" };
  }
  const raw = readFileSync(envPath, "utf-8");
  const lines = raw.split("\n").filter((l) => l.startsWith("TAURI_SIGNING_PRIVATE_KEY="));
  const hasValue = lines.some((l) => {
    const val = l.split("=")[1]?.trim() ?? "";
    return val.length > 0 && !val.startsWith("#");
  });
  return {
    pass: !hasValue,
    detail: hasValue
      ? "SECURITY VIOLATION: TAURI_SIGNING_PRIVATE_KEY is set in .env.local. Remove it immediately."
      : "not set (correct)",
  };
});

// ── Print results table ───────────────────────────────────────────────────────
const COL = 45;
console.log("\nMaintafox Environment Preflight Check");
console.log("\u2500".repeat(60));
console.log(`${"Check".padEnd(COL)} Result`);
console.log("\u2500".repeat(60));

let anyFail = false;
for (const c of checks) {
  const icon = c.pass ? "PASS" : "FAIL";
  const color = c.pass ? "\x1b[32m" : "\x1b[31m";
  const reset = "\x1b[0m";
  console.log(`${c.name.padEnd(COL)} ${color}${icon}${reset}  ${c.detail}`);
  if (!c.pass) anyFail = true;
}

console.log("\u2500".repeat(60));
if (anyFail) {
  console.log("\n\x1b[31mOne or more checks failed. Resolve the items above.\x1b[0m\n");
  process.exit(1);
} else {
  console.log("\n\x1b[32mAll checks passed.\x1b[0m\n");
}

/// <reference types="node" />
import { readFileSync } from "fs";
import { resolve } from "path";
import { fileURLToPath } from "url";
import { dirname } from "path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const confPath = resolve(__dirname, "../src-tauri/tauri.conf.json");
const conf = JSON.parse(readFileSync(confPath, "utf8"));

type AuditResult = { pass: boolean; message: string };
const results: AuditResult[] = [];

function check(condition: boolean, message: string): void {
  results.push({ pass: condition, message });
}

// ─── Window constraints ────────────────────────────────────────────────────

const win = conf?.app?.windows?.[0];
check(win?.visible === false, "window.visible must be false (shows after startup sequence)");
check((win?.minWidth ?? 0) >= 1024, "window.minWidth must be >= 1024");
check((win?.minHeight ?? 0) >= 600, "window.minHeight must be >= 600");
check(win?.label === "main", "window.label must be 'main'");

// ─── CSP ───────────────────────────────────────────────────────────────────

const csp: string = conf?.app?.security?.csp ?? "";
check(csp.includes("default-src 'self'"), "CSP must include default-src 'self'");
check(csp.includes("connect-src") && csp.includes("ipc:"), "CSP must allow ipc: for Tauri bridge");
check(
  csp.includes("https://api.maintafox.systems"),
  "CSP connect-src must allow https://api.maintafox.systems",
);
check(!csp.includes("unsafe-eval"), "CSP must NOT include unsafe-eval");

// ─── Identity ──────────────────────────────────────────────────────────────

check(typeof conf?.version === "string", "version must be a string");
check(
  conf?.identifier === "systems.maintafox.desktop",
  "identifier must be 'systems.maintafox.desktop'",
);

// ─── Capabilities audit ────────────────────────────────────────────────────

const capPath = resolve(__dirname, "../src-tauri/capabilities/default.json");
let cap: { permissions?: string[] } = {};
try {
  cap = JSON.parse(readFileSync(capPath, "utf8"));
} catch {
  check(false, "capabilities/default.json must exist and be valid JSON");
}

const ALLOWED_PERMISSIONS = new Set([
  "core:default",
  "shell:allow-open",
  "dialog:default",
  "fs:default",
]);

const actual = new Set(cap.permissions ?? []);
for (const perm of ALLOWED_PERMISSIONS) {
  check(actual.has(perm), `Required capability permission present: '${perm}'`);
}

for (const perm of actual) {
  if (!ALLOWED_PERMISSIONS.has(perm)) {
    check(false, `Unexpected capability permission: '${perm}'`);
  }
}

// ─── Report ────────────────────────────────────────────────────────────────

let failures = 0;
for (const r of results) {
  const icon = r.pass ? "\u2713" : "\u2717";
  console.log(`  ${icon}  ${r.message}`);
  if (!r.pass) failures++;
}
console.log(`\n${results.length} checks, ${failures} failures`);
process.exit(failures > 0 ? 1 : 0);

#!/usr/bin/env tsx
/**
 * check-rbac-parity.ts
 *
 * Permanent CI gate for desktop RBAC:
 * 1. Generated artifacts match the registry (stale check).
 * 2. No permission-name string literals outside the allowlist.
 * 3. Every enforcement / gate reference resolves to the registry.
 * 4. Every registry entry has intentional usage, and declared usage
 *    agrees with discovered backend/frontend references (catalog-only
 *    requires zero enforcement references).
 *
 * Exit 0 on success, 1 on failure.
 */

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const REGISTRY_PATH = path.join(ROOT, "shared/rbac/permission-registry.json");
const RUST_OUT = path.join(ROOT, "src-tauri/src/rbac/permissions_generated.rs");
const TS_OUT = path.join(ROOT, "shared/rbac/permissions.generated.ts");

const LITERAL_ALLOWLIST = new Set([
  "shared/rbac/permission-registry.json",
  "shared/rbac/permissions.generated.ts",
  "src-tauri/src/rbac/permissions_generated.rs",
  "scripts/generate-permissions.ts",
  "scripts/check-rbac-parity.ts",
  "scripts/rewrite-permission-literals.ts",
]);

interface PermissionEntry {
  name: string;
  usage: "backend" | "frontend" | "both" | "catalog-only";
  usage_justification?: string;
  aliases: string[];
}

interface Registry {
  permissions: PermissionEntry[];
}

function walk(dir: string, exts: Set<string>, out: string[]): void {
  if (!fs.existsSync(dir)) return;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    const rel = path.relative(ROOT, full).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      if (
        entry.name === "node_modules" ||
        entry.name === "target" ||
        entry.name === "dist" ||
        entry.name === ".git" ||
        entry.name === "coverage"
      ) {
        continue;
      }
      walk(full, exts, out);
      continue;
    }
    const ext = path.extname(entry.name);
    if (exts.has(ext)) out.push(rel);
  }
}

function loadRegistry(): Registry {
  return JSON.parse(fs.readFileSync(REGISTRY_PATH, "utf-8")) as Registry;
}

function constName(name: string): string {
  return name.replace(/\./g, "_").toUpperCase();
}

function checkGeneratedFresh(errors: string[]): void {
  const beforeRust = fs.readFileSync(RUST_OUT, "utf-8");
  const beforeTs = fs.readFileSync(TS_OUT, "utf-8");
  execFileSync(
    process.execPath,
    [
      path.join(ROOT, "node_modules/tsx/dist/cli.mjs"),
      path.join(__dirname, "generate-permissions.ts"),
    ],
    {
      cwd: ROOT,
      stdio: "pipe",
    },
  );
  const afterRust = fs.readFileSync(RUST_OUT, "utf-8");
  const afterTs = fs.readFileSync(TS_OUT, "utf-8");
  // Restore is unnecessary if identical; if different, fail and keep regenerated? Prefer fail and restore.
  if (beforeRust !== afterRust || beforeTs !== afterTs) {
    fs.writeFileSync(RUST_OUT, beforeRust, "utf-8");
    fs.writeFileSync(TS_OUT, beforeTs, "utf-8");
    errors.push("Generated permission artifacts are stale. Run: pnpm rbac:generate");
  }
}

function scanLiterals(
  registry: Registry,
  errors: string[],
): {
  backendRefs: Set<string>;
  frontendRefs: Set<string>;
} {
  const names = new Set(registry.permissions.map((p) => p.name));
  const aliases = new Set(registry.permissions.flatMap((p) => p.aliases));
  const allKnown = new Set([...names, ...aliases]);
  // Match "domain.action" and "domain.action.sub"
  const litRe = /["']([a-z]{2,12}(?:\.[a-z0-9_]+){1,3})["']/g;

  const files: string[] = [];
  walk(path.join(ROOT, "src"), new Set([".ts", ".tsx"]), files);
  walk(path.join(ROOT, "shared"), new Set([".ts", ".tsx", ".json"]), files);
  walk(path.join(ROOT, "src-tauri/src"), new Set([".rs"]), files);
  walk(path.join(ROOT, "scripts"), new Set([".ts", ".mjs", ".js"]), files);

  const backendRefs = new Set<string>();
  const frontendRefs = new Set<string>();

  for (const rel of files) {
    if (LITERAL_ALLOWLIST.has(rel)) continue;
    // Historical migrations may retain old permission string seeds — allowlisted by path prefix.
    if (rel.startsWith("src-tauri/src/migrations/")) continue;
    // Docs are not enforcement.
    if (rel.startsWith("docs/")) continue;

    const abs = path.join(ROOT, rel);
    const text = fs.readFileSync(abs, "utf-8");
    const isRust = rel.endsWith(".rs");
    const isFrontend = rel.startsWith("src/") || rel.startsWith("shared/");

    // Constant references (allowed): permissions::EQ_VIEW / P.EQ_VIEW / crate::rbac::permissions::EQ_VIEW
    const constRefRe = isRust
      ? /(?:permissions|permissions_generated)::([A-Z][A-Z0-9_]+)/g
      : /\bP\.([A-Z][A-Z0-9_]+)/g;

    let m: RegExpExecArray | null;
    while ((m = constRefRe.exec(text)) !== null) {
      const cname = m[1];
      const perm = [...names].find((n) => constName(n) === cname);
      if (!perm) continue;
      if (isRust) backendRefs.add(perm);
      if (isFrontend) frontendRefs.add(perm);
    }

    // Also detect check_permission / require_permission with constants already handled above.
    // Literal scan:
    litRe.lastIndex = 0;
    while ((m = litRe.exec(text)) !== null) {
      const candidate = m[1];
      if (!allKnown.has(candidate)) continue;
      // Skip if this is inside a comment documenting an old name? Still forbidden.
      errors.push(
        `Forbidden permission literal ${JSON.stringify(candidate)} in ${rel}:${lineOf(text, m.index)} — use generated constant`,
      );
    }
  }

  return { backendRefs, frontendRefs };
}

function lineOf(text: string, index: number): number {
  return text.slice(0, index).split(/\r?\n/).length;
}

function checkUsageParity(
  registry: Registry,
  backendRefs: Set<string>,
  frontendRefs: Set<string>,
  errors: string[],
): void {
  for (const p of registry.permissions) {
    const be = backendRefs.has(p.name);
    const fe = frontendRefs.has(p.name);
    if (p.usage === "catalog-only") {
      if (be || fe) {
        errors.push(
          `Permission ${p.name} is catalog-only but referenced in ${be ? "backend" : ""}${be && fe ? "+" : ""}${fe ? "frontend" : ""}`,
        );
      }
      if (!p.usage_justification?.trim()) {
        errors.push(`Permission ${p.name} is catalog-only without usage_justification`);
      }
      continue;
    }
    if (p.usage === "backend" && !be) {
      errors.push(
        `Permission ${p.name} declares usage=backend but has no backend constant reference`,
      );
    }
    if (p.usage === "frontend" && !fe) {
      errors.push(
        `Permission ${p.name} declares usage=frontend but has no frontend constant reference`,
      );
    }
    if (p.usage === "both" && (!be || !fe)) {
      errors.push(
        `Permission ${p.name} declares usage=both but missing ${!be ? "backend" : "frontend"} constant reference`,
      );
    }
    if (p.usage === "backend" && fe) {
      // Frontend may still gate on backend-only for UX; allow FE extra, but prefer both.
      // Soft: no error.
    }
  }

  // Every discovered ref must be in registry
  for (const name of [...backendRefs, ...frontendRefs]) {
    if (!registry.permissions.some((p) => p.name === name)) {
      errors.push(`Enforced permission ${name} is missing from the canonical registry`);
    }
  }
}

function main(): void {
  const errors: string[] = [];
  if (!fs.existsSync(REGISTRY_PATH)) {
    console.error("Missing permission-registry.json");
    process.exit(1);
  }
  if (!fs.existsSync(RUST_OUT) || !fs.existsSync(TS_OUT)) {
    console.error("Generated artifacts missing. Run: pnpm rbac:generate");
    process.exit(1);
  }

  const registry = loadRegistry();
  checkGeneratedFresh(errors);
  const { backendRefs, frontendRefs } = scanLiterals(registry, errors);
  checkUsageParity(registry, backendRefs, frontendRefs, errors);

  if (errors.length > 0) {
    console.error(`RBAC parity failed with ${errors.length} issue(s):\n`);
    for (const e of errors) console.error(`  - ${e}`);
    process.exit(1);
  }

  console.log(
    `RBAC parity OK — ${registry.permissions.length} permissions, BE refs=${backendRefs.size}, FE refs=${frontendRefs.size}`,
  );
}

main();

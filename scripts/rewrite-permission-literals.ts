#!/usr/bin/env tsx
/**
 * rewrite-permission-literals.ts
 *
 * One-shot / maintenance helper: replace permission string literals with
 * generated constants across src/, shared/, and src-tauri/src (excluding
 * migrations and generated files).
 *
 * Usage: pnpm tsx scripts/rewrite-permission-literals.ts
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const REGISTRY_PATH = path.join(ROOT, "shared/rbac/permission-registry.json");

interface PermissionEntry {
  name: string;
  aliases: string[];
}

interface Registry {
  permissions: PermissionEntry[];
}

function constName(name: string): string {
  return name.replace(/\./g, "_").toUpperCase();
}

function walk(dir: string, exts: Set<string>, out: string[]): void {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (["node_modules", "target", "dist", ".git"].includes(entry.name)) continue;
      walk(full, exts, out);
      continue;
    }
    if (exts.has(path.extname(entry.name))) out.push(full);
  }
}

function ensureTsImport(content: string): string {
  if (content.includes("permissions.generated") || content.includes("@shared/rbac/permissions")) {
    return content;
  }
  const importLine = `import { P } from "@shared/rbac/permissions.generated";\n`;
  // Prefer after existing imports
  const match = content.match(/^(?:import[\s\S]*?\n)+/);
  if (match) {
    return `${match[0]}${importLine}${content.slice(match[0].length)}`;
  }
  return `${importLine}${content}`;
}

function ensureRustUse(content: string): string {
  if (content.includes("rbac::permissions") || content.includes("use crate::rbac::permissions")) {
    return content;
  }
  // Insert after module-level use block or at top after comments
  const useLine = "use crate::rbac::permissions;\n";
  const crateUse = content.match(/^((?:\/\/.*\n|#!\[.*\]\n|\s*\n)*)/);
  if (crateUse) {
    return `${crateUse[0]}${useLine}${content.slice(crateUse[0].length)}`;
  }
  return `${useLine}${content}`;
}

function main(): void {
  const registry = JSON.parse(fs.readFileSync(REGISTRY_PATH, "utf-8")) as Registry;
  const aliasToCanonical = new Map<string, string>();
  for (const p of registry.permissions) {
    aliasToCanonical.set(p.name, p.name);
    for (const a of p.aliases) aliasToCanonical.set(a, p.name);
  }
  // Longest names first so di.create.own beats di.create
  const allNames = [...aliasToCanonical.keys()].sort((a, b) => b.length - a.length);

  const tsFiles: string[] = [];
  const rsFiles: string[] = [];
  walk(path.join(ROOT, "src"), new Set([".ts", ".tsx"]), tsFiles);
  walk(path.join(ROOT, "shared"), new Set([".ts", ".tsx"]), tsFiles);
  walk(path.join(ROOT, "src-tauri/src"), new Set([".rs"]), rsFiles);

  let changed = 0;

  for (const file of tsFiles) {
    const rel = path.relative(ROOT, file).replace(/\\/g, "/");
    if (rel.includes("permissions.generated")) continue;
    if (rel.includes("permission-registry")) continue;
    let text = fs.readFileSync(file, "utf-8");
    const original = text;
    let touched = false;
    for (const name of allNames) {
      const canonical = aliasToCanonical.get(name)!;
      const c = constName(canonical);
      const re = new RegExp(`(["'])${name.replace(/\./g, "\\.")}\\1`, "g");
      if (re.test(text)) {
        text = text.replace(re, `P.${c}`);
        touched = true;
      }
    }
    if (touched) {
      text = ensureTsImport(text);
      fs.writeFileSync(file, text, "utf-8");
      changed++;
      console.log(`TS  ${rel}`);
    } else if (text !== original) {
      // no-op
    }
  }

  for (const file of rsFiles) {
    const rel = path.relative(ROOT, file).replace(/\\/g, "/");
    if (rel.includes("permissions_generated")) continue;
    if (rel.includes("migrations/")) continue;
    let text = fs.readFileSync(file, "utf-8");
    let touched = false;
    for (const name of allNames) {
      const canonical = aliasToCanonical.get(name)!;
      const c = constName(canonical);
      const re = new RegExp(`"${name.replace(/\./g, "\\.")}"`, "g");
      if (re.test(text)) {
        // Avoid replacing inside comments that are just documentation? Still replace.
        text = text.replace(re, `permissions::${c}`);
        touched = true;
      }
    }
    if (touched) {
      text = ensureRustUse(text);
      fs.writeFileSync(file, text, "utf-8");
      changed++;
      console.log(`RS  ${rel}`);
    }
  }

  console.log(`Rewrote ${changed} files`);
}

main();

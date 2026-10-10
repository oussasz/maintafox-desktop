#!/usr/bin/env node
/**
 * Trims Rust build caches that otherwise grow without bound.
 *
 * Everything removed here is build output that cargo regenerates on demand —
 * no source, database, or git data is touched. What it removes:
 *   1. Ad-hoc `target-*` dirs (one-off CARGO_TARGET_DIRs from agents/checks),
 *      in the repo root and src-tauri/, for the main checkout and every
 *      .claude/worktrees/* checkout.
 *   2. Non-standard subdirs nested inside a target dir that are themselves
 *      target dirs (e.g. target/agent-cargo-check, target/settings-check).
 *   3. Superseded artifacts of OUR crates (maintafox / maintafox_lib) in
 *      <profile>/deps, build and .fingerprint — every code change leaves a new
 *      hashed test binary behind; only the newest few are kept.
 *   4. Loose `*.rcgu.o` object files left in deps/ by interrupted links.
 *   5. <profile>/incremental when it exceeds the size limit (cargo then does
 *      one non-incremental rebuild of our crate; dependencies are untouched).
 *
 * Usage:
 *   node scripts/clean-build-cache.mjs            # clean
 *   node scripts/clean-build-cache.mjs --dry-run  # report only
 *   node scripts/clean-build-cache.mjs --force    # run even while cargo is running
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUR_CRATES = ["maintafox", "maintafox_lib"];
const KEEP_HASHES_PER_CRATE = 4;
const INCREMENTAL_LIMIT_BYTES = 8 * 1024 ** 3;
const LOOSE_OBJECT_MIN_AGE_MS = 24 * 60 * 60 * 1000;
const STANDARD_TARGET_CHILDREN = new Set([
  "debug",
  "release",
  "tmp",
  "doc",
  "package",
  "cargo-timings",
  "rust-analyzer",
  "flycheck0",
]);
// Cross-compilation triples (x86_64-pc-windows-msvc, aarch64-apple-darwin, ...).
const TRIPLE_RE = /-(windows|linux|darwin|apple|android|wasi|unknown)\b/;

const dryRun = process.argv.includes("--dry-run");
const force = process.argv.includes("--force");

function sizeOf(p) {
  let stat;
  try {
    stat = fs.lstatSync(p);
  } catch {
    return 0;
  }
  if (!stat.isDirectory()) return stat.size;
  let total = 0;
  for (const entry of fs.readdirSync(p, { withFileTypes: true })) {
    total += sizeOf(path.join(p, entry.name));
  }
  return total;
}

function gb(bytes) {
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

let freed = 0;
function remove(p, reason) {
  const bytes = sizeOf(p);
  freed += bytes;
  console.log(`${dryRun ? "[dry-run] " : ""}remove ${gb(bytes).padStart(9)}  ${path.relative(REPO_ROOT, p)}  (${reason})`);
  if (!dryRun) fs.rmSync(p, { recursive: true, force: true });
}

function listDirs(p) {
  try {
    return fs.readdirSync(p, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name);
  } catch {
    return [];
  }
}

function cargoIsRunning() {
  try {
    if (process.platform === "win32") {
      return /cargo\.exe/i.test(execSync('tasklist /FI "IMAGENAME eq cargo.exe" /NH', { encoding: "utf8" }));
    }
    return execSync("pgrep -x cargo || true", { encoding: "utf8" }).trim() !== "";
  } catch {
    return false;
  }
}

/** Keeps the newest KEEP_HASHES_PER_CRATE hash generations of each of our crates. */
function pruneHashedArtifacts(dir, { dirsOnly }) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  const generations = new Map(); // crate -> Map(hash -> { mtime, paths[] })
  for (const entry of entries) {
    if (dirsOnly !== entry.isDirectory()) continue;
    const m = entry.name.match(/^(?:lib)?([a-z_-]+)-([0-9a-f]{16})(?:\.|$)/);
    if (!m) continue;
    const crate = m[1].replace(/-/g, "_");
    if (!OUR_CRATES.includes(crate)) continue;
    const full = path.join(dir, entry.name);
    if (!generations.has(crate)) generations.set(crate, new Map());
    const byHash = generations.get(crate);
    const gen = byHash.get(m[2]) ?? { mtime: 0, paths: [] };
    gen.mtime = Math.max(gen.mtime, fs.statSync(full).mtimeMs);
    gen.paths.push(full);
    byHash.set(m[2], gen);
  }
  for (const [crate, byHash] of generations) {
    const stale = [...byHash.values()].sort((a, b) => b.mtime - a.mtime).slice(KEEP_HASHES_PER_CRATE);
    for (const gen of stale) for (const p of gen.paths) remove(p, `superseded ${crate} build`);
  }
}

function pruneLooseObjects(depsDir) {
  let names;
  try {
    names = fs.readdirSync(depsDir);
  } catch {
    return;
  }
  const now = Date.now();
  for (const name of names) {
    if (!name.endsWith(".rcgu.o")) continue;
    const full = path.join(depsDir, name);
    if (now - fs.statSync(full).mtimeMs > LOOSE_OBJECT_MIN_AGE_MS) remove(full, "leftover link object");
  }
}

function cleanProfile(profileDir) {
  pruneHashedArtifacts(path.join(profileDir, "deps"), { dirsOnly: false });
  pruneHashedArtifacts(path.join(profileDir, "build"), { dirsOnly: true });
  pruneHashedArtifacts(path.join(profileDir, ".fingerprint"), { dirsOnly: true });
  pruneLooseObjects(path.join(profileDir, "deps"));
  const incremental = path.join(profileDir, "incremental");
  if (sizeOf(incremental) > INCREMENTAL_LIMIT_BYTES) {
    remove(incremental, `incremental cache over ${gb(INCREMENTAL_LIMIT_BYTES)}`);
  }
}

function cleanTargetDir(targetDir) {
  for (const child of listDirs(targetDir)) {
    const full = path.join(targetDir, child);
    if (STANDARD_TARGET_CHILDREN.has(child)) {
      if (child === "debug" || child === "release") cleanProfile(full);
      continue;
    }
    if (TRIPLE_RE.test(child)) {
      for (const profile of ["debug", "release"]) cleanProfile(path.join(full, profile));
      continue;
    }
    // A nested dir holding its own debug/ or release/ is a stray CARGO_TARGET_DIR.
    if (fs.existsSync(path.join(full, "debug")) || fs.existsSync(path.join(full, "release"))) {
      remove(full, "nested ad-hoc target dir");
    }
  }
}

function cleanCheckout(checkoutRoot) {
  for (const base of [checkoutRoot, path.join(checkoutRoot, "src-tauri")]) {
    for (const name of listDirs(base)) {
      if (name.startsWith("target-")) remove(path.join(base, name), "ad-hoc target dir");
    }
  }
  cleanTargetDir(path.join(checkoutRoot, "src-tauri", "target"));
}

if (!dryRun && !force && cargoIsRunning()) {
  console.error("cargo is running — a build may be using these files. Re-run when it finishes (or pass --force).");
  process.exit(1);
}

const checkouts = [REPO_ROOT];
const worktreesDir = path.join(REPO_ROOT, ".claude", "worktrees");
for (const name of listDirs(worktreesDir)) checkouts.push(path.join(worktreesDir, name));

for (const checkout of checkouts) cleanCheckout(checkout);

console.log(`\n${dryRun ? "Would free" : "Freed"} ${gb(freed)}.`);

#!/usr/bin/env node
// Append the entity types this desktop stages (shared/sync/entity-registry.json, sync_status
// "synced" or "staged_only") to the control-plane sync allowlist, then refresh the desktop copy.
//
// The allowlist is append-only (maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md §2): existing
// entries are never changed or removed. A new entry gets `append_only` from the type's conflict_policy;
// a type already listed with another scope or append_only flag is an error and nothing is written
// (rule in scripts/sync-allowlist-merge.mjs).
//
// Usage: node scripts/sync-allowlist-append.mjs [path/to/maintafox-vendor-console]
//        (default: ../maintafox-vendor-console next to this repository)
// Prints the entity types it added.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { appendStagedTypes } from "./sync-allowlist-merge.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const consoleRepo = resolve(process.argv[2] ?? join(repo, "..", "maintafox-vendor-console"));
const serverPath = join(consoleRepo, "shared", "sync-entity-registry.v2.json");
const copyPath = join(repo, "shared", "sync", "server-entity-allowlist.v2.json");
const registryPath = join(repo, "shared", "sync", "entity-registry.json");

const readJson = (path) => JSON.parse(readFileSync(path, "utf8").replace(/^﻿/, ""));

const registry = readJson(registryPath);
const server = readJson(serverPath);
const { added, problems } = appendStagedTypes(registry, server);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}

const text = `${JSON.stringify(server, null, 2)}\n`;
writeFileSync(serverPath, text);
writeFileSync(copyPath, text);
console.log(added.length ? `added ${added.length}:\n${added.join("\n")}` : "nothing to add");

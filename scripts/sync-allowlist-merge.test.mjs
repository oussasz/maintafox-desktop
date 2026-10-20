import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { appendStagedTypes } from "./sync-allowlist-merge.mjs";

const entity = (entity_type, conflict_policy, extra = {}) => ({
  entity_type,
  scope: "tenant",
  conflict_policy,
  sync_status: "synced",
  ...extra,
});

describe("appendStagedTypes", () => {
  it("gives each new entry append_only from the type's conflict_policy", () => {
    const registry = {
      entities: [
        entity("work_orders", "last_write_wins"),
        entity("audit_events", "append_only"),
        entity("drafts", "append_only", { sync_status: "staged_only" }),
      ],
    };
    const server = { entity_types: [] };
    const { added, problems } = appendStagedTypes(registry, server);
    expect(problems).toEqual([]);
    expect(server.entity_types).toEqual([
      { entity_type: "work_orders", scope: "tenant", append_only: false },
      { entity_type: "audit_events", scope: "tenant", append_only: true },
      { entity_type: "drafts", scope: "tenant", append_only: true },
    ]);
    expect(added).toEqual([
      "work_orders (tenant)",
      "audit_events (tenant, append-only)",
      "drafts (tenant, append-only)",
    ]);
  });

  it("skips types the desktop does not stage and never rewrites an existing entry", () => {
    const registry = {
      entities: [
        entity("local_cache", "append_only", { sync_status: "local_only" }),
        entity("articles", "last_write_wins"),
      ],
    };
    const existing = { entity_type: "articles", scope: "tenant", append_only: false };
    const server = { entity_types: [existing] };
    const { added, problems } = appendStagedTypes(registry, server);
    expect(added).toEqual([]);
    expect(problems).toEqual([]);
    expect(server.entity_types).toEqual([existing]);
  });

  it("reports an existing entry whose scope or append_only flag disagrees, or that lacks the flag", () => {
    const registry = {
      entities: [
        entity("a", "append_only"),
        entity("b", "last_write_wins"),
        entity("c", "append_only"),
        entity("d", "last_write_wins", { scope: "user" }),
      ],
    };
    const server = {
      entity_types: [
        { entity_type: "a", scope: "tenant", append_only: false },
        { entity_type: "b", scope: "tenant", append_only: true },
        { entity_type: "c", scope: "tenant" },
        { entity_type: "d", scope: "tenant", append_only: false },
      ],
    };
    const before = structuredClone(server);
    const { added, problems } = appendStagedTypes(registry, server);
    expect(added).toEqual([]);
    expect(problems).toEqual([
      "a: allowlist append_only false, desktop conflict_policy append_only",
      "b: allowlist append_only true, desktop conflict_policy last_write_wins",
      "c: allowlist entry has no append_only flag (desktop: true)",
      "d: allowlist scope tenant, desktop scope user",
    ]);
    expect(server).toEqual(before);
  });

  it("flags exactly the desktop registry's append_only types when built from scratch", () => {
    const registry = JSON.parse(
      readFileSync(resolve(process.cwd(), "shared/sync/entity-registry.json"), "utf8"),
    );
    const server = { entity_types: [] };
    const { problems } = appendStagedTypes(registry, server);
    expect(problems).toEqual([]);
    const staged = registry.entities.filter(
      (e) => e.sync_status === "synced" || e.sync_status === "staged_only",
    );
    expect(server.entity_types).toHaveLength(staged.length);
    for (const e of staged) {
      const entry = server.entity_types.find((s) => s.entity_type === e.entity_type);
      expect(entry).toEqual({
        entity_type: e.entity_type,
        scope: e.scope,
        append_only: e.conflict_policy === "append_only",
      });
    }
    expect(server.entity_types.some((s) => s.append_only)).toBe(true);
  });
});

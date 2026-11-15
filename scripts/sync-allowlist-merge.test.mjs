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

  it("copies the signing contract onto new and existing entries when it is stricter or equal", () => {
    const authorFields = [{ field: "approved_by_sync_id", guards: ["approved_at"] }];
    const registry = {
      entities: [
        entity("permits", "last_write_wins", { author_fields: authorFields }),
        entity("roles", "last_write_wins", { authority: "test.authority.roles" }),
        entity("notes", "last_write_wins"),
        entity("events", "append_only", { author_fields: authorFields }),
      ],
    };
    const server = {
      entity_types: [
        { entity_type: "permits", scope: "tenant", append_only: false },
        {
          entity_type: "roles",
          scope: "tenant",
          append_only: false,
          authority: "test.authority.roles",
        },
        { entity_type: "notes", scope: "tenant", append_only: false },
      ],
    };
    const { added, updated, problems } = appendStagedTypes(registry, server);
    expect(problems).toEqual([]);
    expect(added).toEqual(["events (tenant, append-only)"]);
    expect(updated).toEqual(["permits (signing contract)"]);
    expect(server.entity_types).toEqual([
      { entity_type: "permits", scope: "tenant", append_only: false, author_fields: authorFields },
      {
        entity_type: "roles",
        scope: "tenant",
        append_only: false,
        authority: "test.authority.roles",
      },
      { entity_type: "notes", scope: "tenant", append_only: false },
      { entity_type: "events", scope: "tenant", append_only: true, author_fields: authorFields },
    ]);
  });

  it("never weakens the server's signing contract: a dropped authority, author or guard is a problem", () => {
    const registry = {
      entities: [
        entity("notes", "last_write_wins"),
        entity("permits", "last_write_wins", {
          author_fields: [{ field: "approved_by_sync_id", guards: [] }],
        }),
      ],
    };
    const server = {
      entity_types: [
        {
          entity_type: "notes",
          scope: "tenant",
          append_only: false,
          authority: "test.authority.users",
          author_fields: [{ field: "made_by_sync_id", guards: [] }],
        },
        {
          entity_type: "permits",
          scope: "tenant",
          append_only: false,
          author_fields: [{ field: "approved_by_sync_id", guards: ["approved_at"] }],
        },
      ],
    };
    const before = structuredClone(server);
    const { added, updated, problems } = appendStagedTypes(registry, server);
    expect(added).toEqual([]);
    expect(updated).toEqual([]);
    expect(problems).toEqual([
      "notes: allowlist authority test.authority.users, desktop none",
      "notes: author field made_by_sync_id is in the allowlist, not on the desktop",
      "permits: guard approved_at of approved_by_sync_id is in the allowlist, not on the desktop",
    ]);
    expect(server).toEqual(before);
  });

  it("adds the desktop's server-only types once, never as staged types", () => {
    const registry = {
      entities: [entity("articles", "last_write_wins")],
      server_only_types: [{ entity_type: "user_signing_keys", scope: "tenant" }],
    };
    const server = { entity_types: [] };
    const first = appendStagedTypes(registry, server);
    expect(first.problems).toEqual([]);
    expect(first.added).toEqual(["articles (tenant)", "user_signing_keys (tenant, server-only)"]);
    expect(server.entity_types[1]).toEqual({
      entity_type: "user_signing_keys",
      scope: "tenant",
      append_only: false,
      server_only: true,
    });
    const again = appendStagedTypes(registry, server);
    expect(again).toEqual({ added: [], updated: [], problems: [] });
  });

  it("refuses a server-only type the desktop stages or an allowlist entry that is not server-only", () => {
    const registry = {
      entities: [
        entity("user_signing_keys", "last_write_wins"),
        entity("keys2", "last_write_wins"),
      ],
      server_only_types: [
        { entity_type: "user_signing_keys", scope: "tenant" },
        { entity_type: "k3", scope: "tenant" },
      ],
    };
    const server = {
      entity_types: [
        { entity_type: "keys2", scope: "tenant", append_only: false, server_only: true },
        { entity_type: "k3", scope: "tenant", append_only: false },
      ],
    };
    const before = structuredClone(server);
    const { added, updated, problems } = appendStagedTypes(registry, server);
    expect(added).toEqual([]);
    expect(updated).toEqual([]);
    expect(problems).toEqual([
      "keys2: server-only in the allowlist, staged by the desktop",
      "user_signing_keys: server-only on the desktop, which also stages it",
      "k3: allowlist entry is not server-only, desktop server-only type",
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
    const serverOnly = registry.server_only_types ?? [];
    expect(server.entity_types).toHaveLength(staged.length + serverOnly.length);
    for (const e of staged) {
      const entry = server.entity_types.find((s) => s.entity_type === e.entity_type);
      expect(entry).toEqual({
        entity_type: e.entity_type,
        scope: e.scope,
        append_only: e.conflict_policy === "append_only",
        ...(e.authority ? { authority: e.authority } : {}),
        ...(e.author_fields?.length ? { author_fields: e.author_fields } : {}),
      });
    }
    expect(server.entity_types.some((s) => s.append_only)).toBe(true);
    expect(serverOnly.map((s) => s.entity_type)).toContain("user_signing_keys");
    expect(server.entity_types.some((s) => s.author_fields?.length)).toBe(true);
  });
});

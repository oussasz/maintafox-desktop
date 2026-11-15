import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import {
  eventLabel,
  fieldLabel,
  hasEventLabel,
  payloadChanges,
  payloadChangesUnavailable,
  payloadLines,
  targetTypeLabel,
} from "@/components/activity/activity-labels";
import enActivity from "@/i18n/locale-data/en/activity.json";
import frActivity from "@/i18n/locale-data/fr/activity.json";

function tFor(catalog: unknown): TFunction {
  return ((key: string, opts?: { defaultValue?: string }) => {
    const value = key
      .split(".")
      .reduce<unknown>(
        (node, part) => (node as Record<string, unknown> | undefined)?.[part],
        catalog,
      );
    return typeof value === "string" ? value : (opts?.defaultValue ?? key);
  }) as unknown as TFunction;
}

describe("event and action names", () => {
  it("translates known codes in FR and EN", () => {
    expect(eventLabel(tFor(enActivity), "login.success")).toBe("Signed in");
    expect(eventLabel(tFor(frActivity), "login.success")).toBe("Connexion réussie");
    expect(eventLabel(tFor(enActivity), "pm.occurrence.missed")).toBe("PM occurrence missed");
    expect(eventLabel(tFor(frActivity), "wo.closed")).toBe("OT clôturé");
  });

  it("keeps a readable name for a code this version does not know, never a blank", () => {
    expect(eventLabel(tFor(enActivity), "sync.future_thing.done")).toBe("sync future thing done");
  });

  it("has the same event names in FR and EN", () => {
    const en = Object.keys((enActivity.labels as Record<string, object>)["event"] ?? {});
    const fr = Object.keys((frActivity.labels as Record<string, object>)["event"] ?? {});
    expect(en.length).toBeGreaterThan(50);
    expect(fr.sort()).toEqual(en.sort());
  });

  it("names every audit event type the backend declares, in FR and EN", () => {
    // The list of event types is `audit::event_type` in src-tauri/src/audit/mod.rs; read it from
    // there so a new constant without a label fails here instead of showing its raw code.
    const source = readFileSync(
      join(__dirname, "..", "..", "..", "..", "src-tauri", "src", "audit", "mod.rs"),
      "utf8",
    );
    const block = /pub mod event_type \{([\s\S]*?)\r?\n\}/.exec(source)?.[1] ?? "";
    const codes = [...block.matchAll(/pub const \w+: &str = "([^"]+)";/g)].map((m) => m[1] ?? "");
    expect(codes.length).toBeGreaterThan(20);
    expect(codes).toContain("logout");

    const events = (catalog: { labels: Record<string, unknown> }) =>
      catalog.labels["event"] as Record<string, string>;
    for (const catalog of [enActivity, frActivity]) {
      const missing = codes.filter((code) => !(code.replace(/\./g, "__") in events(catalog)));
      expect(missing).toEqual([]);
    }
    expect(eventLabel(tFor(enActivity), "logout")).toBe("Sign-out");
    expect(eventLabel(tFor(frActivity), "logout")).toBe("Déconnexion");
  });

  it("names every audit action the backend writes, including the OT changes it copies (L5-09)", () => {
    const srcRoot = join(__dirname, "..", "..", "..", "..", "src-tauri", "src");
    const files = (readdirSync(srcRoot, { recursive: true }) as string[]).filter(
      (f) =>
        f.endsWith(".rs") &&
        !/tests?\.rs$|_tests|[\\/]tests[\\/]|migrations|permissions_generated/.test(f),
    );
    const codes = new Set<string>();
    const literal = /(?:action_code|event_type): "([a-z_]+(?:\.[a-z_]+)+)"/g;
    const constant = /const (?:AUDIT_\w+|\w+_EVENT): &str = "([a-z_]+(?:\.[a-z_]+)+)"/g;
    for (const file of files) {
      const text = readFileSync(join(srcRoot, file), "utf8");
      for (const m of text.matchAll(literal)) codes.add(m[1] ?? "");
      for (const m of text.matchAll(constant)) codes.add(m[1] ?? "");
      // `record_wo_change_with_diff` copies OT changes to the audit trail as `wo.<action>`.
      for (const block of text.split("record_wo_change_with_diff(").slice(1)) {
        // Every string literal of the `action:` expression, whatever its form ("x".into(),
        // `if moved { "a" } else { "b" }.into()`, `if c { "a".into() } else { "b".into() }`).
        const body = block.slice(0, block.indexOf(".await;"));
        const start = body.indexOf("action: ");
        if (start < 0) continue;
        const rest = body.slice(start);
        const expr = rest.slice(0, rest.search(/\n\s+actor_id:/));
        for (const m of expr.matchAll(/"([a-z_]+)"/g)) codes.add(`wo.${m[1] ?? ""}`);
      }
    }
    codes.delete("test.action");
    expect(codes.size).toBeGreaterThan(20);
    expect(codes).toContain("wo.planned");
    expect(codes).toContain("wo.site_moved");
    expect(codes).toContain("wo.equipment_changed");
    expect(codes).toContain("wo.shutdown_required_set");
    for (const catalog of [enActivity, frActivity]) {
      const missing = [...codes].filter((code) => !hasEventLabel(tFor(catalog), code));
      expect(missing).toEqual([]);
    }
    expect(eventLabel(tFor(frActivity), "asset.moved")).toBe("Équipement déplacé");
    expect(eventLabel(tFor(frActivity), "permit.approved")).toBe("Permis approuvé");
  });

  it("names objects and changed fields in French, readable fallback otherwise", () => {
    const fr = tFor(frActivity);
    expect(targetTypeLabel(fr, "work_order")).toBe("OT");
    expect(fieldLabel(fr, "closed_at")).toBe("Date de clôture");
    expect(fieldLabel(fr, "some_new_field")).toBe("some new field");
    expect(hasEventLabel(fr, "sync.future_thing.done")).toBe(false);
  });

  it("reads field changes from an event's details, never from older events", () => {
    const change = { field: "closed_at", old: null, new: "2026-10-28T18:26:00Z" };
    expect(payloadChanges({ changes: [change] })).toEqual([change]);
    expect(payloadChanges({ detail: { changes: [change] } })).toEqual([change]);
    expect(payloadChanges(JSON.stringify({ changes: [change] }))).toEqual([change]);
    expect(payloadChanges({ summary: "Work order closed" })).toEqual([]);
    expect(payloadChanges("not json")).toEqual([]);
    expect(payloadChanges(null)).toEqual([]);
    expect(payloadChangesUnavailable({ changes_unavailable: true })).toBe(true);
    expect(payloadChangesUnavailable({ detail: { changes_unavailable: true } })).toBe(true);
    expect(payloadChangesUnavailable({ changes: [change] })).toBe(false);
  });

  it("shows detail lines with French keys, without the changes, copy markers or sync ids", () => {
    const fr = tFor(frActivity);
    const lines = payloadLines(
      {
        summary: "Work order closed",
        reason: "fuite",
        source: "wo_change_events",
        isolation_sync_id: "abc",
        changes: [{ field: "status", old: "a", new: "b" }],
        detail: { changes: [] },
      },
      fr,
      { hideSummary: true },
    );
    expect(lines.map((l) => l.key)).toEqual(["Motif"]);
  });
});

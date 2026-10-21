import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import { eventLabel } from "@/components/activity/activity-labels";
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
});

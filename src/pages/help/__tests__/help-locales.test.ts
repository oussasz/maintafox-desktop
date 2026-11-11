/** Every guide of the registry has its texts in French and English, with the same keys. */
import { describe, expect, it } from "vitest";

import enHelp from "@/i18n/locale-data/en/help.json";
import frHelp from "@/i18n/locale-data/fr/help.json";

import { HELP_GUIDES } from "../help-guides";

type Json = Record<string, unknown>;

function collectKeys(obj: Json, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value !== null && typeof value === "object" ? collectKeys(value as Json, path) : [path];
  });
}

describe.each([
  ["fr", frHelp],
  ["en", enHelp],
])("help texts (%s)", (_lang, help) => {
  it.each(HELP_GUIDES.map((g) => [g.id, g] as const))("guide %s is complete", (id, guide) => {
    const mod = (help.modules as Record<string, Record<string, string>>)[id];
    expect(mod, `modules.${id}`).toBeDefined();
    expect(mod?.["title"]).toBeTruthy();
    expect(mod?.["purpose"]).toBeTruthy();
    const stepKeys = Object.keys(mod ?? {}).filter((k) => /^step\d+$/.test(k));
    expect(stepKeys.sort()).toEqual(
      Array.from({ length: guide.steps }, (_, i) => `step${i + 1}`).sort(),
    );
  });
});

describe("help texts parity", () => {
  it("has the same keys in French and English", () => {
    expect(collectKeys(enHelp as Json).sort()).toEqual(collectKeys(frHelp as Json).sort());
  });
});

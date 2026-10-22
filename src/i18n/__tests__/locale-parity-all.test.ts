// Every locale file, eager and lazy, has the same keys in French and English: a text added in
// one language only shows as a key or a foreign sentence in the other.

import { describe, expect, it } from "vitest";

type Json = Record<string, unknown>;

const modules = import.meta.glob<Json>(
  ["../fr/*.json", "../en/*.json", "../locale-data/*/*.json"],
  {
    eager: true,
    import: "default",
  },
);

function collectKeys(obj: Json, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return value !== null && typeof value === "object" && !Array.isArray(value)
      ? collectKeys(value as Json, path)
      : [path];
  });
}

/** `../fr/errors.json` → "fr/errors"; `../locale-data/fr/ot.json` → "locale-data/fr/ot". */
const frFiles = Object.keys(modules).filter((p) => /\/fr\/[^/]+\.json$/.test(p));

describe("fr ↔ en parity, every namespace", () => {
  it("finds the locale files", () => {
    expect(frFiles.length).toBeGreaterThan(30);
  });

  it.each(frFiles)("%s has an English twin with the same keys", (frPath) => {
    const enPath = frPath.replace("/fr/", "/en/");
    const fr = modules[frPath];
    const en = modules[enPath];
    expect(en, `missing ${enPath}`).toBeDefined();
    const frKeys = new Set(collectKeys(fr as Json));
    const enKeys = new Set(collectKeys(en as Json));
    const onlyFr = [...frKeys].filter((k) => !enKeys.has(k));
    const onlyEn = [...enKeys].filter((k) => !frKeys.has(k));
    expect({ onlyFr, onlyEn }).toEqual({ onlyFr: [], onlyEn: [] });
  });

  it("has no English file without a French twin", () => {
    const enOnly = Object.keys(modules)
      .filter((p) => /\/en\/[^/]+\.json$/.test(p))
      .filter((p) => !(p.replace("/en/", "/fr/") in modules));
    expect(enOnly).toEqual([]);
  });
});

import { describe, expect, it } from "vitest";

import ar from "../locales/ar.json";
import en from "../locales/en.json";
import fr from "../locales/fr.json";

function keysOf(node: unknown, prefix = ""): string[] {
  if (typeof node !== "object" || node === null) return [prefix];
  return Object.entries(node).flatMap(([k, v]) => keysOf(v, prefix ? `${prefix}.${k}` : k));
}

function placeholdersOf(text: string): string[] {
  return [...text.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1] ?? "").sort();
}

function valueAt(node: unknown, key: string): string {
  return key
    .split(".")
    .reduce<unknown>((n, part) => (n as Record<string, unknown>)[part], node) as string;
}

describe("mobile locales", () => {
  const frKeys = keysOf(fr).sort();

  it("has the same keys in fr, en and ar", () => {
    expect(keysOf(en).sort()).toEqual(frKeys);
    expect(keysOf(ar).sort()).toEqual(frKeys);
  });

  it("has no empty value", () => {
    for (const [name, resource] of [
      ["fr", fr],
      ["en", en],
      ["ar", ar],
    ] as const) {
      for (const key of frKeys) {
        expect(valueAt(resource, key).trim(), `${name}:${key}`).not.toBe("");
      }
    }
  });

  it("uses the same interpolation placeholders in every language", () => {
    for (const key of frKeys) {
      const expected = placeholdersOf(valueAt(fr, key));
      expect(placeholdersOf(valueAt(en, key)), `en:${key}`).toEqual(expected);
      expect(placeholdersOf(valueAt(ar, key)), `ar:${key}`).toEqual(expected);
    }
  });
});

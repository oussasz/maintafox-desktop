// Ensures translation resources store plain UTF-8 text — never HTML entities.
// Entities like &apos; render literally when strings pass through JS literals or i18n.

import { describe, expect, it } from "vitest";

const localeJsonModules = import.meta.glob<{ default: Record<string, unknown> }>(
  ["../**/*.json", "!../__tests__/**"],
  { eager: true },
);

const HTML_ENTITY_PATTERN = /&(apos|amp|lt|gt|quot|#\d+|#x[0-9a-fA-F]+);/;

function collectStringLeaves(
  obj: Record<string, unknown>,
  prefix = "",
): Array<{ path: string; value: string }> {
  return Object.entries(obj).flatMap(([key, value]) => {
    const fullPath = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      return collectStringLeaves(value as Record<string, unknown>, fullPath);
    }
    if (typeof value === "string") {
      return [{ path: fullPath, value }];
    }
    return [];
  });
}

describe("i18n translation resources — no HTML entities", () => {
  it("does not contain HTML entities in any locale JSON string value", () => {
    const violations: string[] = [];

    for (const [filePath, mod] of Object.entries(localeJsonModules)) {
      const relativePath = filePath.replace(/^\.\.\//, "src/i18n/");
      const rawContent = JSON.stringify(mod.default);
      const rawMatch = rawContent.match(HTML_ENTITY_PATTERN);
      if (rawMatch) {
        violations.push(`${relativePath}: raw JSON contains &${rawMatch[1]};`);
      }

      for (const { path, value } of collectStringLeaves(mod.default)) {
        const match = value.match(HTML_ENTITY_PATTERN);
        if (match) {
          violations.push(`${relativePath} → ${path}: contains &${match[1]};`);
        }
      }
    }

    expect(violations, violations.join("\n")).toEqual([]);
  });
});

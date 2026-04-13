// src/i18n/__tests__/parity.test.ts
// Ensures every eager namespace has identical key trees in fr and en.
// Formats is excluded because its tokens are intentionally locale-specific.

import { describe, it, expect } from "vitest";

// ── Eager namespace imports (fr) ────────────────────────────────
import enAuth from "../en/auth.json";
import enCommon from "../en/common.json";
import enErrors from "../en/errors.json";
import enShell from "../en/shell.json";
import enValidation from "../en/validation.json";
import frAuth from "../fr/auth.json";
import frCommon from "../fr/common.json";
import frErrors from "../fr/errors.json";
import frShell from "../fr/shell.json";
import frValidation from "../fr/validation.json";

// ── Eager namespace imports (en) ────────────────────────────────

// ── Helper: collect all dot-paths from a nested object ──────────
function collectKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      return collectKeys(value as Record<string, unknown>, path);
    }
    return [path];
  });
}

// ── Helper: symmetric diff between two sorted key arrays ────────
function symmetricDiff(a: string[], b: string[]): { onlyA: string[]; onlyB: string[] } {
  const setA = new Set(a);
  const setB = new Set(b);
  return {
    onlyA: a.filter((k) => !setB.has(k)),
    onlyB: b.filter((k) => !setA.has(k)),
  };
}

// ── Parity test suite ───────────────────────────────────────────
const EAGER_PAIRS: [string, Record<string, unknown>, Record<string, unknown>][] = [
  ["common", frCommon as Record<string, unknown>, enCommon as Record<string, unknown>],
  ["auth", frAuth as Record<string, unknown>, enAuth as Record<string, unknown>],
  ["errors", frErrors as Record<string, unknown>, enErrors as Record<string, unknown>],
  ["validation", frValidation as Record<string, unknown>, enValidation as Record<string, unknown>],
  ["shell", frShell as Record<string, unknown>, enShell as Record<string, unknown>],
];

describe("i18n key parity (fr ↔ en)", () => {
  it.each(EAGER_PAIRS)("%s — fr and en have identical key paths", (ns, frJson, enJson) => {
    const frKeys = collectKeys(frJson).sort();
    const enKeys = collectKeys(enJson).sort();
    const diff = symmetricDiff(frKeys, enKeys);

    if (diff.onlyA.length > 0 || diff.onlyB.length > 0) {
      const msg = [
        `[${ns}] key parity mismatch:`,
        diff.onlyA.length > 0 ? `  Only in fr: ${diff.onlyA.join(", ")}` : "",
        diff.onlyB.length > 0 ? `  Only in en: ${diff.onlyB.join(", ")}` : "",
      ]
        .filter(Boolean)
        .join("\n");
      expect.fail(msg);
    }

    // Sanity: both arrays should have the same length if no diff
    expect(frKeys.length).toBe(enKeys.length);
  });
});

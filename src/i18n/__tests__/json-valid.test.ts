// src/i18n/__tests__/json-valid.test.ts
// Validates that JSON files conform to the expected namespace structure:
// - Top-level keys match the defined schema
// - All leaf values are non-empty strings (no stale nulls or objects)

import { describe, it, expect } from "vitest";

// ── Imports ─────────────────────────────────────────────────────
import enAuth from "../en/auth.json";
import enCommon from "../en/common.json";
import frAuth from "../fr/auth.json";
import frCommon from "../fr/common.json";

// ── Helper: assert every leaf value is a non-empty string ───────
function assertLeafStrings(obj: Record<string, unknown>, path = ""): void {
  for (const [key, value] of Object.entries(obj)) {
    const fullPath = path ? `${path}.${key}` : key;
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      assertLeafStrings(value as Record<string, unknown>, fullPath);
    } else {
      expect(typeof value, `${fullPath} should be a string`).toBe("string");
      expect((value as string).length, `${fullPath} should be non-empty`).toBeGreaterThan(0);
    }
  }
}

// ── Top-level key structure ─────────────────────────────────────
const COMMON_TOP_KEYS = ["app", "action", "status", "label", "confirm", "pagination", "time"];
const AUTH_TOP_KEYS = ["login", "logout", "session", "stepUp", "device"];

describe("json-valid: common namespace", () => {
  it("fr/common.json has the expected top-level keys", () => {
    expect(Object.keys(frCommon).sort()).toEqual([...COMMON_TOP_KEYS].sort());
  });

  it("en/common.json has the expected top-level keys", () => {
    expect(Object.keys(enCommon).sort()).toEqual([...COMMON_TOP_KEYS].sort());
  });

  it("all values in fr/common.json are non-empty strings", () => {
    assertLeafStrings(frCommon as Record<string, unknown>);
  });

  it("all values in en/common.json are non-empty strings", () => {
    assertLeafStrings(enCommon as Record<string, unknown>);
  });
});

describe("json-valid: auth namespace", () => {
  it("fr/auth.json has the expected top-level keys", () => {
    expect(Object.keys(frAuth).sort()).toEqual([...AUTH_TOP_KEYS].sort());
  });

  it("en/auth.json has the expected top-level keys", () => {
    expect(Object.keys(enAuth).sort()).toEqual([...AUTH_TOP_KEYS].sort());
  });
});

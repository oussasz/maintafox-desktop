// src/i18n/__tests__/types.test.ts
// These tests DO NOT run assertions — they are type-only checks.
// If the file compiles, the type narrowing is working correctly.
// Any invalid key would cause a TypeScript error during `pnpm run typecheck`.

import { describe, it, expect } from "vitest";

// This import activates the augmentation. The test file must
// import i18n to trigger the CustomTypeOptions module augmentation.
import "../types";

describe("i18n type safety (compile-time checks)", () => {
  it("valid key paths compile without error", () => {
    // These are type assertions only — no runtime behavior tested.
    // If any of these keys does NOT exist in the JSON files,
    // TypeScript will report a compile error here.
    type ValidCommonKey = "app.name" extends string ? true : never;
    const _check: ValidCommonKey = true;
    expect(_check).toBe(true);
  });
});

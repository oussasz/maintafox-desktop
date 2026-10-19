import { readFileSync } from "node:fs";

import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import frPermits from "@/i18n/locale-data/fr/permits.json";

import {
  SEEDED_PERMIT_TYPE_NAMES,
  shippedPermitTypeNameKey,
  usePermitTypeName,
} from "../permit-type-name";

const lang = vi.hoisted(() => ({ current: "fr" }));

function lookup(catalog: unknown, key: string): string {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  if (typeof value !== "string") throw new Error(`missing permits key: ${key}`);
  return value;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => lookup(lang.current === "fr" ? frPermits : enPermits, key),
  }),
}));

describe("shipped permit type names", () => {
  it("lists exactly the names the migration seeds, by code", () => {
    const sql = readFileSync(
      "src-tauri/src/migrations/m20260615_000067_permit_domain_core.rs",
      "utf8",
    );
    const seeded = [
      ...sql.matchAll(/'[0-9a-f-]{36}', '(\w+)', '([^']+)', '[^']*', \d, \d, \d/g),
    ].map((m) => [m[1], m[2]]);
    expect(Object.fromEntries(seeded)).toEqual(SEEDED_PERMIT_TYPE_NAMES);
  });

  it("has a French and an English text for every shipped type", () => {
    for (const code of Object.keys(SEEDED_PERMIT_TYPE_NAMES)) {
      expect(lookup(frPermits, `types.shipped.${code}`).trim()).not.toBe("");
      expect(lookup(enPermits, `types.shipped.${code}`).trim()).not.toBe("");
    }
  });

  it("translates a shipped type whose stored name is still the seeded one", () => {
    lang.current = "fr";
    const { result } = renderHook(() => usePermitTypeName());
    expect(result.current("confined_space", "Confined space")).toBe("Espace confiné");
    expect(result.current("hot_work", " Hot work ")).toBe("Travail par point chaud");
    lang.current = "en";
    const en = renderHook(() => usePermitTypeName());
    expect(en.result.current("confined_space", "Confined space")).toBe("Confined space");
  });

  it("shows a name typed by the plant exactly as stored", () => {
    lang.current = "fr";
    const { result } = renderHook(() => usePermitTypeName());
    // A shipped code renamed by the plant.
    expect(result.current("hot_work", "Permis de feu")).toBe("Permis de feu");
    // A plant's own type that happens to carry a seeded name under another code.
    expect(result.current("my_type", "Hot work")).toBe("Hot work");
    expect(shippedPermitTypeNameKey("toString", "Hot work")).toBeNull();
    expect(shippedPermitTypeNameKey(null, "Hot work")).toBeNull();
  });
});

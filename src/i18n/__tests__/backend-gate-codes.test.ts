// Every GATE_* refusal code the backend can send has a French and an English text under
// `errors:gateError`: an untranslated code would reach the user as a bare internal code.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import enErrors from "../en/errors.json";
import frErrors from "../fr/errors.json";

function rustFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) return rustFiles(full);
    return name.endsWith(".rs") ? [full] : [];
  });
}

function gateCodesInBackend(): string[] {
  const codes = new Set<string>();
  for (const file of rustFiles(join(process.cwd(), "src-tauri", "src"))) {
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(/"\{?(GATE_[A-Z0-9_]+)/g)) {
      if (match[1]) codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("backend GATE codes are translated", () => {
  const codes = gateCodesInBackend();

  it("finds the codes in the backend sources", () => {
    expect(codes.length).toBeGreaterThan(50);
  });

  it("has a French text for each", () => {
    const gate = frErrors.gateError as Record<string, string>;
    expect(codes.filter((c) => !gate[c])).toEqual([]);
  });

  it("has an English text for each", () => {
    const gate = enErrors.gateError as Record<string, string>;
    expect(codes.filter((c) => !gate[c])).toEqual([]);
  });
});

/**
 * Every conflict kind the sync centre explains has its text in French and in English (a missing
 * one would show the raw code).
 */
import { describe, expect, it } from "vitest";

import en from "@/i18n/en/shell.json";
import fr from "@/i18n/fr/shell.json";

import { KNOWN_CONFLICT_KINDS } from "../sync-conflict-display";

describe("sync conflict kinds", () => {
  for (const [lang, shell] of [
    ["en", en],
    ["fr", fr],
  ] as const) {
    it(`are all explained in ${lang}`, () => {
      const kinds = (shell as { sync: { conflictKind: Record<string, string> } }).sync.conflictKind;
      const missing = [...KNOWN_CONFLICT_KINDS].filter((k) => !kinds[k]?.trim());
      expect(missing).toEqual([]);
    });
  }

  it("include the safety rules re-checked on a received row", () => {
    for (const code of [
      "LOTO_NOT_VERIFIED",
      "PERMIT_TWO_PERSON_VERIFY",
      "PERMIT_REQUIRED_TO_START",
      "QUALIFICATION_OVERRIDE_UNAUTHORISED",
      "SYNC_STATE_TIME_MISSING",
      "WO_PART_LINE_HAS_USE",
    ]) {
      expect(KNOWN_CONFLICT_KINDS.has(code)).toBe(true);
    }
  });
});

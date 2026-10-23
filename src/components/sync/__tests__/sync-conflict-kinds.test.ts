/**
 * Every conflict kind the sync centre explains has its text in French and in English (a missing
 * one would show the raw code).
 */
import { describe, expect, it } from "vitest";

import en from "@/i18n/en/shell.json";
import fr from "@/i18n/fr/shell.json";

import {
  KNOWN_APPLY_REASONS,
  KNOWN_CONFLICT_KINDS,
  KNOWN_WAIT_REASONS,
} from "../sync-conflict-display";

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

    it(`explain every refused-row reason and every wait in ${lang}`, () => {
      const sync = (
        shell as {
          sync: {
            applyReason: Record<string, string>;
            waiting: { own: Record<string, string>; state: Record<string, string> };
          };
        }
      ).sync;
      expect([...KNOWN_APPLY_REASONS].filter((k) => !sync.applyReason[k]?.trim())).toEqual([]);
      expect([...KNOWN_WAIT_REASONS, "other"].filter((k) => !sync.waiting.own[k]?.trim())).toEqual(
        [],
      );
      expect(
        ["conflict", "refused", "not_received", "ready", "chain_unresolved"].filter(
          (k) => !sync.waiting.state[k]?.trim(),
        ),
      ).toEqual([]);
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
      "WO_PERMIT_SAFETY_ORIGIN_CHANGED",
      "WO_COMPLETION_GATE",
      "WO_CLOSE_GATE",
      "WO_TASK_AFTER_COMPLETION",
      "WO_FAILURE_DETAIL_AFTER_CLOSE",
      "WO_LABOUR_AFTER_CLOSE",
      "WO_TOOL_AFTER_CLOSE",
      "WO_VERIFICATION_WRONG_STATUS",
      "WO_SELF_VERIFICATION",
      "WO_VERIFIER_RECORDED_LABOUR",
    ]) {
      expect(KNOWN_CONFLICT_KINDS.has(code)).toBe(true);
    }
  });
});

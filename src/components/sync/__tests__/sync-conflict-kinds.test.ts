/**
 * Every conflict kind the sync centre explains has its text in French and in English (a missing
 * one would show the raw code).
 */
import { describe, expect, it } from "vitest";

import en from "@/i18n/en/shell.json";
import fr from "@/i18n/fr/shell.json";

import {
  COUNT_HISTORY_ENTITY_TYPES,
  KNOWN_APPLY_REASONS,
  KNOWN_CONFLICT_KINDS,
  KNOWN_WAIT_REASONS,
  PLANNING_CONFLICT_KINDS,
  REVIEW_ONLY_CONFLICT_KINDS,
  SETTLED_AT_ORIGIN_CONFLICT_KINDS,
  conflictActions,
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
      "GRANT_BY_UNAUTHORISED_ADMIN",
      "INSP_ANOMALY_PERMIT_REVIEW_CLEARED",
      "INSP_ANOMALY_LINK_CHANGED",
      "INSP_ANOMALY_DECISION_INVALID",
      "WO_COMPLETION_GATE",
      "WO_CLOSE_GATE",
      "WO_TASK_AFTER_COMPLETION",
      "WO_FAILURE_DETAIL_AFTER_CLOSE",
      "WO_LABOUR_AFTER_CLOSE",
      "WO_TOOL_AFTER_CLOSE",
      "WO_VERIFICATION_WRONG_STATUS",
      "WO_SELF_VERIFICATION",
      "WO_VERIFIER_RECORDED_LABOUR",
      "WO_MANDATORY_TASK_REMOVED",
      "WO_CANCEL_PERMIT_GATE",
      "PERMIT_ENDED_BEFORE_START",
      "PERMIT_ENDED_DURING_WORK",
    ]) {
      expect(KNOWN_CONFLICT_KINDS.has(code)).toBe(true);
    }
  });

  it("offer only keep this PC's plan and escalate on the planning four-eyes refusals", () => {
    const planning = [
      "PLANNING_WEEK_FROZEN",
      "PLANNING_BREAK_IN_APPROVAL_REQUIRED",
      "PLANNING_BREAK_IN_SELF_APPROVAL",
      "PLANNING_BREAK_IN_APPROVER_UNAUTHORISED",
      "PLANNING_BREAK_IN_UNSIGNED",
      "WEEK_UNFREEZE_UNDECIDED",
      "WEEK_UNFREEZE_SAME_PERSON",
      "WEEK_UNFREEZE_UNAUTHORISED",
    ];
    expect([...PLANNING_CONFLICT_KINDS].sort()).toEqual([...planning].sort());
    for (const kind of planning) {
      expect(KNOWN_CONFLICT_KINDS.has(kind)).toBe(true);
      expect(REVIEW_ONLY_CONFLICT_KINDS.has(kind)).toBe(false);
      for (const triage of [false, true]) {
        expect(conflictActions(kind, triage, "work_orders")).toEqual(["accept_local", "escalate"]);
      }
    }
  });

  it("offer only escalate and dismiss on a review-only conflict (the server refuses the others)", () => {
    for (const kind of ["PERMIT_ENDED_BEFORE_START", "PERMIT_ENDED_DURING_WORK"]) {
      expect(REVIEW_ONLY_CONFLICT_KINDS.has(kind)).toBe(true);
      for (const triage of [false, true]) {
        expect(conflictActions(kind, triage, "work_orders")).toEqual(["escalate", "dismiss"]);
      }
    }
    // A count held for a second approval is settled where it is applied: no keep / take / dismiss.
    expect(SETTLED_AT_ORIGIN_CONFLICT_KINDS.has("COUNT_SELF_APPROVAL")).toBe(true);
    expect(conflictActions("COUNT_SELF_APPROVAL", false, "work_orders")).toEqual(["escalate"]);
    expect(conflictActions("COUNT_SELF_APPROVAL", true, "work_orders")).toEqual([
      "retry_later",
      "escalate",
    ]);
    expect(conflictActions("CONCURRENT_EDIT", false, "work_orders")).toEqual([
      "accept_local",
      "accept_remote",
      "dismiss",
    ]);
    expect(conflictActions("CODE_CLASH", true, "work_orders")).toEqual([
      "renumber_local",
      "retry_later",
      "escalate",
      "dismiss",
    ]);
    expect(conflictActions("USERNAME_CLASH", false, "work_orders")).toEqual([
      "accept_remote",
      "dismiss",
    ]);
  });

  it("offer only escalate and dismiss on a refused count movement or its audit link (piece F)", () => {
    // The server accepts only these two when the refused history row is not held here, and the
    // screen cannot tell (`refused_history_row_absent` in src-tauri/src/sync/queries.rs).
    expect([...COUNT_HISTORY_ENTITY_TYPES].sort()).toEqual([
      "inventory_mutation_audit_links",
      "inventory_transactions",
    ]);
    for (const entity of COUNT_HISTORY_ENTITY_TYPES) {
      for (const triage of [false, true]) {
        expect(conflictActions("COUNT_APPROVAL_RECORD_INVALID", triage, entity)).toEqual([
          "escalate",
          "dismiss",
        ]);
      }
    }
    // On the count line itself "take theirs" is the release path: the usual choices stay.
    expect(
      conflictActions("COUNT_APPROVAL_RECORD_INVALID", false, "inventory_count_lines"),
    ).toEqual(["accept_local", "accept_remote", "dismiss"]);
    expect(conflictActions("COUNT_APPROVAL_RECORD_INVALID", true, "inventory_count_lines")).toEqual(
      ["accept_local", "accept_remote", "retry_later", "escalate", "dismiss"],
    );
    // Other kinds on the same history records are unchanged.
    for (const entity of COUNT_HISTORY_ENTITY_TYPES) {
      expect(conflictActions("CONCURRENT_EDIT", true, entity)).toEqual([
        "accept_local",
        "accept_remote",
        "retry_later",
        "escalate",
        "dismiss",
      ]);
      expect(conflictActions("COUNT_SELF_APPROVAL", false, entity)).toEqual(["escalate"]);
      expect(conflictActions("COUNT_APPROVAL_RECORD_CHANGED", false, entity)).toEqual([
        "accept_local",
        "accept_remote",
        "dismiss",
      ]);
    }
  });
});

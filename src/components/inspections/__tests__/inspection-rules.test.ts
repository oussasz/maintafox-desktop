import { describe, expect, it } from "vitest";

import type { InspectionRunnerCheckpoint } from "@shared/ipc-types";

import {
  answerChoices,
  canComplete,
  describeBand,
  firstOpenIndex,
  parseReading,
  pendingSyncCount,
  predictNumericStatus,
} from "../inspection-rules";

const limits = (
  normal_min: number | null,
  normal_max: number | null,
  warning_min: number | null,
  warning_max: number | null,
) => ({ normal_min, normal_max, warning_min, warning_max });

describe("predictNumericStatus (mirrors results.rs eval_numeric_status)", () => {
  it("classifies two-sided bands", () => {
    const cp = limits(2, 4, 1, 5);
    expect(predictNumericStatus(3, cp)).toBe("pass");
    expect(predictNumericStatus(4.5, cp)).toBe("warning");
    expect(predictNumericStatus(5, cp)).toBe("warning");
    expect(predictNumericStatus(0.5, cp)).toBe("fail");
  });

  it("supports one-sided limits", () => {
    const cp = limits(null, 70, null, 85);
    expect(predictNumericStatus(65, cp)).toBe("pass");
    expect(predictNumericStatus(80, cp)).toBe("warning");
    expect(predictNumericStatus(90, cp)).toBe("fail");
  });

  it("treats a plain measurement as pass and a normal-only breach as warning", () => {
    expect(predictNumericStatus(1e6, limits(null, null, null, null))).toBe("pass");
    expect(predictNumericStatus(9, limits(10, null, null, null))).toBe("warning");
  });
});

describe("parseReading", () => {
  it("accepts comma and dot decimals", () => {
    expect(parseReading("12,5")).toBe(12.5);
    expect(parseReading(" -3.25 ")).toBe(-3.25);
    expect(parseReading(".5")).toBe(0.5);
  });
  it("rejects anything that is not a number", () => {
    expect(parseReading("")).toBeNull();
    expect(parseReading("12a")).toBeNull();
    expect(parseReading("1,2,3")).toBeNull();
  });
});

describe("describeBand", () => {
  it("describes each band shape", () => {
    expect(describeBand(null, null)).toBeNull();
    expect(describeBand(1, 2)).toEqual({ kind: "between", min: 1, max: 2 });
    expect(describeBand(null, 70)).toEqual({ kind: "below", min: null, max: 70 });
    expect(describeBand(5, null)).toEqual({ kind: "above", min: 5, max: null });
  });
});

describe("answerChoices", () => {
  it("offers the right buttons per check type", () => {
    expect(answerChoices("pass_fail")).toEqual(["pass", "fail", "not_accessible", "not_done"]);
    expect(answerChoices("observation")).toContain("warning");
    expect(answerChoices("numeric")).toEqual(["not_accessible", "not_done"]);
  });
});

function cp(
  partial: Partial<Pick<InspectionRunnerCheckpoint, "missing" | "sync_state">>,
): InspectionRunnerCheckpoint {
  return {
    checkpoint: {
      id: 1,
      entity_sync_id: "c",
      template_version_id: 1,
      sequence_order: 1,
      asset_id: null,
      component_id: null,
      checkpoint_code: "C",
      description: null,
      check_type: "pass_fail",
      measurement_unit: null,
      normal_min: null,
      normal_max: null,
      warning_min: null,
      warning_max: null,
      requires_photo: false,
      requires_comment_on_exception: false,
      row_version: 1,
    },
    asset_code: null,
    asset_name: null,
    result: null,
    evidence: [],
    open_anomalies: [],
    sync_state: partial.sync_state ?? "synced",
    missing: partial.missing ?? [],
  };
}

describe("round helpers", () => {
  it("completes only when nothing is missing", () => {
    expect(canComplete([])).toBe(false);
    expect(canComplete([cp({}), cp({})])).toBe(true);
    expect(canComplete([cp({}), cp({ missing: ["photo"] })])).toBe(false);
  });

  it("counts the offline queue and errors", () => {
    expect(
      pendingSyncCount([
        cp({ sync_state: "waiting" }),
        cp({ sync_state: "waiting" }),
        cp({ sync_state: "conflict" }),
        cp({ sync_state: "synced" }),
      ]),
    ).toEqual({ waiting: 2, errors: 1 });
  });

  it("opens on the first check that still needs something", () => {
    expect(firstOpenIndex([cp({}), cp({ missing: ["result"] })])).toBe(1);
    expect(firstOpenIndex([cp({})])).toBe(0);
  });
});

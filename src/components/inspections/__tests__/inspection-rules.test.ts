import { describe, expect, it } from "vitest";

import type { InspectionAnomalyRow, InspectionRunnerCheckpoint } from "@shared/ipc-types";

import {
  type CheckRowInput,
  type ScheduleForm,
  answerChoices,
  canComplete,
  describeBand,
  firstOpenIndex,
  followUpDraft,
  isoToLocalInput,
  kpiDrillDown,
  parseReading,
  pendingSyncCount,
  predictNumericStatus,
  readingNeedsComment,
  roundFilterFor,
  scheduleFormIssues,
  sparklinePoints,
  validateCheckRows,
  violatedLimit,
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
    history: [],
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

describe("violatedLimit and readingNeedsComment (mirror results.rs)", () => {
  const cpLimits = limits(null, 7.5, null, 9);
  it("reports the normal limit inside the warning band and the warning limit beyond it", () => {
    expect(violatedLimit(8.4, cpLimits)).toEqual({ direction: "above", limit: 7.5 });
    expect(violatedLimit(10, cpLimits)).toEqual({ direction: "above", limit: 9 });
    expect(violatedLimit(7, cpLimits)).toBeNull();
    expect(violatedLimit(1, limits(2, 4, 1.5, 5))).toEqual({ direction: "below", limit: 1.5 });
  });
  it("asks a comment only for a numeric reading out of its limits", () => {
    expect(readingNeedsComment("numeric", "warning")).toBe(true);
    expect(readingNeedsComment("numeric", "fail")).toBe(true);
    expect(readingNeedsComment("numeric", "pass")).toBe(false);
    expect(readingNeedsComment("pass_fail", "fail")).toBe(false);
  });
});

describe("roundFilterFor and kpiDrillDown", () => {
  const period = { from: "2026-09-14T10:00:00Z", to: "2026-10-14T10:00:00Z" };
  it("maps status groups, timing and search to the backend filter", () => {
    expect(roundFilterFor({ status: "todo", timing: null, period, search: "" })).toEqual({
      statuses: ["scheduled", "released", "in_progress"],
    });
    expect(roundFilterFor({ status: null, timing: "overdue", period, search: " pump " })).toEqual({
      statuses: null,
      overdue_only: true,
      search: "pump",
    });
    const due = roundFilterFor({ status: null, timing: "period", period, search: "" });
    expect(due.statuses).not.toContain("cancelled");
    expect(due.due_from).toBe(period.from);
    expect(due.due_to).toBe(period.to);
    expect(
      roundFilterFor({ status: "missed", timing: "period", period, search: "" }).statuses,
    ).toEqual(["missed"]);
  });
  it("sends every KPI tile to the list behind its figure", () => {
    expect(kpiDrillDown("due")).toMatchObject({ tab: "rounds", status: null, timing: "period" });
    expect(kpiDrillDown("done")).toMatchObject({ status: "finished", timing: "period" });
    expect(kpiDrillDown("missed")).toMatchObject({ status: "missed", timing: "period" });
    expect(kpiDrillDown("overdue")).toMatchObject({ timing: "overdue" });
    expect(kpiDrillDown("compliance")).toMatchObject({ timing: "period" });
    expect(kpiDrillDown("anomalies")).toMatchObject({ tab: "anomalies", anomalyStatus: "open" });
  });
});

describe("validateCheckRows (mirrors queries::validate_drafts)", () => {
  const row = (partial: Partial<CheckRowInput>): CheckRowInput => ({
    code: "C",
    checkType: "numeric",
    normalMin: "",
    normalMax: "",
    warningMin: "",
    warningMax: "",
    ...partial,
  });
  it("names the row and the reason", () => {
    const issues = validateCheckRows([
      row({ code: "" }),
      row({ code: "P1" }),
      row({ code: "p1" }),
      row({ code: "T", normalMax: "abc" }),
      row({ code: "U", normalMin: "5", normalMax: "1" }),
      row({ code: "V", normalMin: "0", normalMax: "10", warningMin: "1", warningMax: "12" }),
      row({ code: "W", normalMax: "7,5", warningMax: "9" }),
    ]);
    expect(issues.get(0)).toEqual([{ code: "codeRequired" }]);
    expect(issues.has(1)).toBe(false);
    expect(issues.get(2)).toEqual([{ code: "codeDuplicate" }]);
    expect(issues.get(3)).toEqual([{ code: "limitInvalid", field: "normalMax" }]);
    expect(issues.get(4)).toEqual([{ code: "limitsOrder" }]);
    expect(issues.get(5)).toEqual([{ code: "warningBand" }]);
    expect(issues.has(6)).toBe(false);
  });
  it("ignores limits of non-numeric rows", () => {
    expect(validateCheckRows([row({ checkType: "pass_fail", normalMax: "x" })]).size).toBe(0);
  });
});

describe("followUpDraft", () => {
  const t = (key: string, opts: Record<string, unknown>) =>
    `${key}${Object.keys(opts).length ? JSON.stringify(opts) : ""}`;
  const base: InspectionAnomalyRow = {
    anomaly: {
      id: 7,
      round_id: 3,
      result_id: 9,
      anomaly_type: "threshold_breach",
      severity: 4,
      description: "x",
      linked_di_id: null,
      linked_work_order_id: null,
      requires_permit_review: false,
      resolution_status: "open",
      routing_decision: null,
      entity_sync_id: "a",
      row_version: 1,
    },
    round_status: "in_progress",
    round_scheduled_at: "2026-10-12T06:00:00Z",
    round_completed_at: null,
    template_code: "RND-1",
    template_name: "Pump house",
    checkpoint_id: 1,
    checkpoint_code: "PRESS",
    checkpoint_description: "Pressure",
    check_type: "numeric",
    measurement_unit: "bar",
    normal_min: null,
    normal_max: 7.5,
    warning_min: null,
    warning_max: 9,
    asset_id: 4,
    asset_code: "P-101",
    asset_name: "Feed pump",
    asset_has_location: true,
    result_status: "warning",
    numeric_value: 8.4,
    text_value: null,
    result_comment: "Gauge vibrating",
    recorded_at: "2026-10-12T06:10:00Z",
    violation_direction: "above",
    violation_limit: 7.5,
    linked_di_code: null,
    linked_di_title: null,
    linked_di_status: null,
    linked_wo_code: null,
    linked_wo_title: null,
  };
  const fmtDate = (v: string | null) => v ?? "";
  it("writes a readable title with equipment, reading and limit", () => {
    const d = followUpDraft(base, t, (v) => String(v), fmtDate);
    expect(d.title).toBe(
      'followUp.title.above{"label":"Pressure","asset":"P-101","value":"8.4 bar","limit":"7.5 bar"}',
    );
    expect(d.description).toContain("followUp.description.round");
    expect(d.description).toContain('"comment":"Gauge vibrating"');
    expect(d.description).toContain('"band":"≤ 7.5 bar"');
  });
  it("falls back to the finding title without equipment", () => {
    const d = followUpDraft(
      {
        ...base,
        asset_code: null,
        numeric_value: null,
        violation_direction: null,
        violation_limit: null,
        result_status: "fail",
        check_type: "pass_fail",
      },
      t,
      String,
      fmtDate,
    );
    expect(d.title.startsWith("followUp.title.findingNoAsset")).toBe(true);
  });
  it("titles a missing photo", () => {
    const d = followUpDraft(
      {
        ...base,
        anomaly: { ...base.anomaly, anomaly_type: "missing_mandatory_photo" },
        numeric_value: null,
      },
      t,
      String,
      fmtDate,
    );
    expect(d.title.startsWith("followUp.title.photo{")).toBe(true);
  });
});

describe("scheduleFormIssues (mirrors scheduling::validate_schedule_input)", () => {
  const form = (partial: Partial<ScheduleForm>): ScheduleForm => ({
    code: "S1",
    name: "Daily pumps",
    templateId: "3",
    triggerKind: "calendar",
    intervalValue: "1",
    intervalUnit: "day",
    anchorLocal: "2026-10-15T06:00",
    meterId: null,
    meterInterval: "",
    meterStart: "",
    assigneeId: 1,
    teamId: null,
    graceHours: "24",
    horizonDays: "14",
    ...partial,
  });
  it("accepts a complete calendar schedule", () => {
    expect(scheduleFormIssues(form({}))).toEqual({});
  });
  it("flags missing fields and ranges", () => {
    expect(
      scheduleFormIssues(
        form({
          code: " ",
          assigneeId: null,
          graceHours: "0",
          intervalValue: "1.5",
          horizonDays: "400",
        }),
      ),
    ).toEqual({
      code: "required",
      assignee: "assigneeRequired",
      graceHours: "graceRange",
      intervalValue: "intervalRequired",
      horizonDays: "horizonRange",
    });
  });
  it("checks the meter fields of a meter schedule", () => {
    expect(
      scheduleFormIssues(
        form({ triggerKind: "meter", meterId: null, meterInterval: "0", meterStart: "x" }),
      ),
    ).toEqual({
      meterId: "meterRequired",
      meterInterval: "meterIntervalRequired",
      meterStart: "notNumber",
    });
    expect(
      scheduleFormIssues(
        form({ triggerKind: "meter", meterId: 5, meterInterval: "500", teamId: 2 }),
      ),
    ).toEqual({});
  });
});

describe("isoToLocalInput and sparklinePoints", () => {
  it("formats an instant for the local date-time field", () => {
    expect(isoToLocalInput(null)).toBe("");
    expect(isoToLocalInput("nope")).toBe("");
    expect(isoToLocalInput("2026-10-15T06:30:00")).toBe("2026-10-15T06:30");
  });
  it("draws a series in its box, oldest left", () => {
    expect(sparklinePoints([], 100, 10)).toBe("");
    expect(sparklinePoints([5], 100, 10)).toBe("50,5");
    expect(sparklinePoints([0, 10], 100, 10)).toBe("0,10 100,0");
  });
});

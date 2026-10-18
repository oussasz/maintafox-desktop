import { describe, expect, it } from "vitest";

import { PM_EXECUTION_RESULTS } from "@/components/pm/pm-labels";
import {
  PM_TRIGGER_FORM_START,
  allowedExecutionResults,
  buildTriggerJson,
  calendarOccurrenceTone,
  canLogExecution,
  matchesDueWindow,
  parseRequiredParts,
  partsWithoutNecessity,
  parseTriggerJson,
  requiredPartsJson,
} from "@/components/pm/pm-model";
import type { PmOccurrenceRow } from "@/services/pm-service";

const NOW = new Date("2026-10-14T12:00:00Z");

function due(dueAt: string): PmOccurrenceRow {
  return { due_at: dueAt, status: "scheduled" } as PmOccurrenceRow;
}

describe("pm-model", () => {
  it("offers execution logging only where the backend completes the occurrence", () => {
    expect(canLogExecution({ status: "in_progress" })).toBe(true);
    expect(canLogExecution({ status: "scheduled" })).toBe(false);
    expect(allowedExecutionResults("in_progress", PM_EXECUTION_RESULTS)).toEqual([
      "completed_no_findings",
      "completed_with_findings",
      "deferred",
    ]);
    expect(allowedExecutionResults("completed", PM_EXECUTION_RESULTS)).toEqual([]);
  });

  it("uses the backend due-KPI windows so a tile and its list agree", () => {
    expect(matchesDueWindow(due("2026-10-14T08:00:00Z"), "overdue", NOW)).toBe(true);
    expect(matchesDueWindow(due("2026-10-14T08:00:00Z"), "today", NOW)).toBe(true);
    expect(matchesDueWindow(due("2026-10-14T18:00:00Z"), "overdue", NOW)).toBe(false);
    expect(matchesDueWindow(due("2026-10-20T18:00:00Z"), "next7", NOW)).toBe(true);
    expect(matchesDueWindow(due("2026-10-22T18:00:00Z"), "next7", NOW)).toBe(false);
  });

  it("builds and reads a recurring trigger on a chosen asset meter", () => {
    const json = buildTriggerJson({
      ...PM_TRIGGER_FORM_START,
      strategy: "meter",
      meter: { assetMeterId: 7, meterType: null, interval: 250, startReading: 1000 },
    });
    expect(JSON.parse(json)).toEqual({
      asset_meter_id: 7,
      interval_value: 250,
      start_reading: 1000,
    });
    const back = parseTriggerJson("meter", json);
    expect(back.meter).toEqual({
      assetMeterId: 7,
      meterType: null,
      interval: 250,
      startReading: 1000,
    });
  });

  it("writes tolerance and the whichever-comes-first meter block of a calendar version", () => {
    const json = buildTriggerJson({
      ...PM_TRIGGER_FORM_START,
      strategy: "floating",
      intervalUnit: "month",
      intervalValue: 1,
      orMeter: { assetMeterId: null, meterType: "HOURS", interval: 500, startReading: null },
      tolerance: { unit: "percent", early: 10, late: 10 },
    });
    expect(JSON.parse(json)).toEqual({
      interval_unit: "month",
      interval_value: 1,
      meter: { meter_type: "HOURS", interval_value: 500 },
      tolerance: { unit: "percent", early: 10, late: 10 },
    });
    const back = parseTriggerJson("floating", json);
    expect(back.orMeter?.meterType).toBe("HOURS");
    expect(back.tolerance).toEqual({ unit: "percent", early: 10, late: 10 });
    // Legacy operating-hours versions read as the HOURS meter type.
    expect(
      parseTriggerJson("meter", '{"meter_source":"operating_hours","interval_value":100}').meter,
    ).toMatchObject({ meterType: "HOURS", interval: 100 });
  });

  it("keeps quantities of structured required parts and plain codes of older versions", () => {
    const parts = parseRequiredParts(
      '[{"article_code":"FLT-10","quantity":2,"required":true},{"code":"JOINT","required":false},"GRAISSE"]',
    );
    expect(parts).toEqual([
      { articleCode: "FLT-10", quantity: 2, required: true },
      { articleCode: "JOINT", quantity: null, required: false },
      // An entry that never said: unanswered, the planner must choose before saving.
      { articleCode: "GRAISSE", quantity: null, required: null },
    ]);
    expect(partsWithoutNecessity(parts)).toEqual(["GRAISSE"]);
    const answered = parts.map((p) => ({ ...p, required: p.required ?? true }));
    expect(partsWithoutNecessity(answered)).toEqual([]);
    expect(requiredPartsJson(answered)).toBe(
      '[{"article_code":"FLT-10","quantity":2,"required":true},{"article_code":"JOINT","required":false},{"article_code":"GRAISSE","required":true}]',
    );
    expect(requiredPartsJson([])).toBeNull();
  });

  it("counts an occurrence as overdue only past its tolerance window", () => {
    const occ = {
      due_at: "2026-10-13T00:00:00Z",
      window_end_at: "2026-10-16T23:59:59Z",
      status: "scheduled",
    } as PmOccurrenceRow;
    expect(matchesDueWindow(occ, "overdue", NOW)).toBe(false);
    expect(matchesDueWindow(occ, "overdue", new Date("2026-10-17T08:00:00Z"))).toBe(true);
  });

  it("colours a calendar chip overdue only past the tolerance window, like the KPI", () => {
    const inTolerance = {
      due_at: "2026-10-13T00:00:00Z",
      window_end_at: "2026-10-16T23:59:59Z",
      status: "scheduled",
    } as PmOccurrenceRow;
    // Past its due day but inside its window: due (warning), not overdue.
    expect(calendarOccurrenceTone(inTolerance, NOW)).toBe("today");
    expect(calendarOccurrenceTone(inTolerance, new Date("2026-10-17T08:00:00Z"))).toBe("overdue");
    // No window: overdue once the due instant has passed (same as the overdue tile).
    expect(calendarOccurrenceTone(due("2026-10-14T08:00:00Z"), NOW)).toBe("overdue");
    expect(calendarOccurrenceTone(due("2026-10-14T18:00:00Z"), NOW)).toBe("today");
    expect(calendarOccurrenceTone(due("2026-10-20T08:00:00Z"), NOW)).toBe("pipeline");
    expect(
      calendarOccurrenceTone({ ...due("2026-10-01T08:00:00Z"), status: "completed" }, NOW),
    ).toBe("completed");
  });
});

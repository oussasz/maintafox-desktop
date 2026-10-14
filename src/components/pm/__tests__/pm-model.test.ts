import { describe, expect, it } from "vitest";

import { PM_EXECUTION_RESULTS } from "@/components/pm/pm-labels";
import {
  PM_TRIGGER_FORM_START,
  allowedExecutionResults,
  buildTriggerJson,
  canLogExecution,
  matchesDueWindow,
  parseTriggerJson,
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
      assetMeterId: 7,
      meterInterval: 250,
      startReading: 1000,
    });
    expect(JSON.parse(json)).toEqual({
      asset_meter_id: 7,
      interval_value: 250,
      start_reading: 1000,
    });
    const back = parseTriggerJson("meter", json);
    expect(back.assetMeterId).toBe(7);
    expect(back.meterInterval).toBe(250);
    expect(back.startReading).toBe(1000);
  });
});

/**
 * Inspection service (Phase 6.3): each new call reaches its backend command with the right
 * arguments and validates the answer (a malformed answer is refused, never shown).
 */
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  generateInspectionRounds,
  getInspectionAnomalyRow,
  getInspectionKpis,
  listInspectionAnomalyRows,
  listInspectionRoundPage,
  listInspectionSchedules,
  routeInspectionAnomalyToDi,
  routeInspectionAnomalyToWo,
  saveInspectionSchedule,
} from "../inspection-service";

const round = {
  id: 1,
  entity_sync_id: "r1",
  template_id: 2,
  template_version_id: 3,
  scheduled_at: "2026-10-15T06:00:00Z",
  assigned_to_id: 4,
  status: "scheduled",
  started_at: null,
  completed_at: null,
  reviewed_at: null,
  reviewed_by_id: null,
  review_note: null,
  completion_percentage: null,
  row_version: 1,
  schedule_id: 9,
  due_basis: "calendar:2026-10-15T06:00:00Z",
  window_end_at: "2026-10-15T12:00:00Z",
  assigned_team_id: null,
  due_meter_value: null,
  missed_at: null,
};

const summary = {
  round,
  template_code: "RND-1",
  template_name: "Pump house",
  version_no: 1,
  requires_review: false,
  assigned_to_name: "Amina",
  assigned_team_name: null,
  schedule_code: "SCH-1",
  schedule_name: "Daily",
  is_overdue: false,
  reviewed_by_name: null,
  checkpoint_count: 3,
  recorded_count: 0,
  exception_count: 0,
  open_anomaly_count: 0,
  pending_sync_count: 0,
};

const anomaly = {
  id: 7,
  round_id: 1,
  result_id: 8,
  anomaly_type: "threshold_breach",
  severity: 4,
  description: "x",
  linked_di_id: null,
  linked_work_order_id: null,
  requires_permit_review: false,
  resolution_status: "open",
  routing_decision: null,
  entity_sync_id: "a7",
  row_version: 1,
};

const anomalyRow = {
  anomaly,
  round_status: "in_progress",
  round_scheduled_at: "2026-10-15T06:00:00Z",
  round_completed_at: null,
  template_code: "RND-1",
  template_name: "Pump house",
  checkpoint_id: 5,
  checkpoint_code: "PRESS",
  checkpoint_description: "Pressure",
  check_type: "numeric",
  measurement_unit: "bar",
  normal_min: null,
  normal_max: 7.5,
  warning_min: null,
  warning_max: 9,
  asset_id: 10,
  asset_code: "P-101",
  asset_name: "Feed pump",
  asset_has_location: true,
  result_status: "warning",
  numeric_value: 8.4,
  text_value: null,
  result_comment: "Vibrating",
  recorded_at: "2026-10-15T06:10:00Z",
  violation_direction: "above",
  violation_limit: 7.5,
  linked_di_code: null,
  linked_di_title: null,
  linked_di_status: null,
  linked_wo_code: null,
  linked_wo_title: null,
};

const scheduleSummary = {
  schedule: {
    id: 9,
    entity_sync_id: "s9",
    code: "SCH-1",
    name: "Daily",
    template_id: 2,
    trigger_kind: "calendar",
    interval_unit: "day",
    interval_value: 1,
    anchor_at: "2026-10-15T06:00:00Z",
    meter_id: null,
    meter_interval: null,
    meter_start_reading: null,
    assigned_to_id: 4,
    assigned_team_id: null,
    grace_hours: 6,
    horizon_days: 14,
    is_active: true,
    last_generated_at: null,
    row_version: 1,
    created_at: "2026-10-15T05:00:00Z",
    updated_at: "2026-10-15T05:00:00Z",
  },
  template_code: "RND-1",
  template_name: "Pump house",
  assigned_to_name: "Amina",
  assigned_team_name: null,
  meter_name: null,
  meter_unit: null,
  meter_asset_id: null,
  meter_asset_code: null,
  meter_lifetime_reading: null,
  next_due_at: "2026-10-15T06:00:00Z",
  open_round_count: 14,
  overdue_round_count: 0,
  missed_round_count: 0,
};

describe("inspection service (Phase 6.3)", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("lists a page of rounds with its total", async () => {
    mockInvoke.mockResolvedValueOnce({ items: [summary], total: 41 });
    const page = await listInspectionRoundPage({ overdue_only: true, limit: 200, offset: 0 });
    expect(mockInvoke).toHaveBeenCalledWith("list_inspection_round_page", {
      filter: { overdue_only: true, limit: 200, offset: 0 },
    });
    expect(page.total).toBe(41);
    expect(page.items[0]?.schedule_code).toBe("SCH-1");
  });

  it("reads the figures and refuses a malformed answer", async () => {
    const kpis = {
      period_start: "2026-09-15T00:00:00Z",
      period_end: "2026-10-15T00:00:00Z",
      due_count: 4,
      done_count: 2,
      done_on_time_count: 1,
      missed_count: 1,
      overdue_count: 1,
      compliance_pct: 25,
      open_anomaly_count: 1,
    };
    mockInvoke.mockResolvedValueOnce(kpis);
    await expect(getInspectionKpis({ period_days: 30 })).resolves.toEqual(kpis);
    expect(mockInvoke).toHaveBeenCalledWith("get_inspection_kpis", { input: { period_days: 30 } });
    mockInvoke.mockResolvedValueOnce({ ...kpis, due_count: "4" });
    await expect(getInspectionKpis({ period_days: 30 })).rejects.toThrow();
  });

  it("lists and reads anomalies with their context", async () => {
    mockInvoke.mockResolvedValueOnce({ items: [anomalyRow], total: 1 });
    const page = await listInspectionAnomalyRows({ statuses: ["open"] });
    expect(mockInvoke).toHaveBeenCalledWith("list_inspection_anomaly_rows", {
      filter: { statuses: ["open"] },
    });
    expect(page.items[0]?.violation_limit).toBe(7.5);
    mockInvoke.mockResolvedValueOnce(anomalyRow);
    const row = await getInspectionAnomalyRow(7);
    expect(mockInvoke).toHaveBeenLastCalledWith("get_inspection_anomaly_row", { anomalyId: 7 });
    expect(row.asset_code).toBe("P-101");
  });

  it("returns the created request or work order to link it", async () => {
    mockInvoke.mockResolvedValueOnce([
      { id: 55, code: "DI-0042", title: "Pressure high", status: "submitted" },
      { ...anomaly, linked_di_id: 55 },
    ]);
    const [di, updated] = await routeInspectionAnomalyToDi({
      anomaly_id: 7,
      expected_row_version: 1,
      title: "Pressure high",
      description: "Reading 8.4 bar",
    });
    expect(di).toEqual({ id: 55, code: "DI-0042", title: "Pressure high" });
    expect(updated.linked_di_id).toBe(55);
    mockInvoke.mockResolvedValueOnce([
      { id: 66, code: "OT-0007", title: "Pressure high" },
      { ...anomaly, linked_work_order_id: 66 },
    ]);
    const [wo] = await routeInspectionAnomalyToWo({
      anomaly_id: 7,
      expected_row_version: 1,
      type_code: "corrective",
      title: "Pressure high",
      description: null,
    });
    expect(wo.code).toBe("OT-0007");
  });

  it("lists, saves and generates schedules", async () => {
    mockInvoke.mockResolvedValueOnce([scheduleSummary]);
    expect(await listInspectionSchedules()).toHaveLength(1);
    expect(mockInvoke.mock.calls[0]?.[0]).toBe("list_inspection_schedules");

    mockInvoke.mockResolvedValueOnce(scheduleSummary);
    const input = {
      code: "SCH-1",
      name: "Daily",
      template_id: 2,
      trigger_kind: "calendar" as const,
      interval_unit: "day",
      interval_value: 1,
      anchor_at: "2026-10-15T06:00:00Z",
      assigned_to_id: 4,
      grace_hours: 6,
      horizon_days: 14,
      is_active: true,
    };
    const saved = await saveInspectionSchedule(input);
    expect(mockInvoke).toHaveBeenLastCalledWith("save_inspection_schedule", { input });
    expect(saved.open_round_count).toBe(14);

    mockInvoke.mockResolvedValueOnce({
      generated: 3,
      skipped: 0,
      missed: 1,
      round_ids: [1, 2, 3],
      issues: [{ schedule_code: "SCH-2", message: "GATE_INSP_SCHEDULE_METER_INACTIVE: x" }],
    });
    const run = await generateInspectionRounds({});
    expect(mockInvoke).toHaveBeenLastCalledWith("generate_inspection_rounds", { input: {} });
    expect(run.issues[0]?.schedule_code).toBe("SCH-2");
  });
});

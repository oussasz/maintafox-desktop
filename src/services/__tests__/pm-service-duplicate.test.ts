import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/ipc-invoke";

import { PmIpcError, duplicatePmPlan, isReportedPmError } from "../pm-service";

vi.mock("@/lib/ipc-invoke", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

const TS = "2026-10-01T10:00:00Z";

const input = {
  source_plan_id: 1,
  code: "PM-COPY",
  title: "Copie de Graissage pompe",
  target_scope_type: "equipment",
  target_scope_id: 20,
  target_meter_id: null,
};

const plan = {
  id: 7,
  code: "PM-COPY",
  title: "Copie de Graissage pompe",
  description: null,
  asset_scope_type: "equipment",
  asset_scope_id: 20,
  strategy_type: "fixed",
  criticality_value_id: null,
  criticality_code: null,
  criticality_label: null,
  assigned_team_id: null,
  assigned_team_code: null,
  assigned_team_name: null,
  assigned_team_label: null,
  requires_shutdown: 0,
  requires_permit: 0,
  is_active: 1,
  lifecycle_status: "draft",
  current_version_id: null,
  row_version: 1,
  created_at: TS,
  updated_at: TS,
};

const version = {
  id: 700,
  pm_plan_id: 7,
  version_no: 1,
  status: "draft",
  effective_from: TS,
  effective_to: null,
  trigger_definition_json: "{}",
  task_package_json: null,
  required_parts_json: null,
  required_skills_json: null,
  required_tools_json: null,
  estimated_duration_hours: null,
  planned_crew_size: null,
  estimated_labor_cost: null,
  estimated_parts_cost: null,
  estimated_service_cost: null,
  change_reason: "copié de PM-SRC v1",
  row_version: 1,
  created_at: TS,
  updated_at: TS,
};

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("duplicatePmPlan", () => {
  it("sends the input to duplicate_pm_plan without the global toast", async () => {
    mockInvoke.mockResolvedValueOnce({ plan, version, meter_to_choose: true });
    const result = await duplicatePmPlan(input);
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith(
      "duplicate_pm_plan",
      { input },
      { notifyOnError: false },
    );
    expect(result.plan.code).toBe("PM-COPY");
    expect(result.meter_to_choose).toBe(true);
  });

  it("returns a refusal as an error the dialog must say itself (not reported)", async () => {
    mockInvoke.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: "GATE_PM_PLAN_CODE_TAKEN: Another PM plan already uses the code 'PM-COPY'.",
    });
    const err = await duplicatePmPlan(input).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(PmIpcError);
    expect((err as PmIpcError).code).toBe("VALIDATION_FAILED");
    expect((err as PmIpcError).reported).toBe(false);
    expect(isReportedPmError(err)).toBe(false);
    expect((err as PmIpcError).message).toContain("GATE_PM_PLAN_CODE_TAKEN");
  });
});

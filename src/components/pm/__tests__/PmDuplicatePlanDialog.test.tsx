import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  PmDuplicatePlanDialog,
  sourceMeterIdOf,
  versionToCopy,
} from "@/components/pm/PmDuplicatePlanDialog";
import type { PmAssetMeter, PmPlanRow } from "@/services/pm-service";
import type { PmPlanVersion } from "@shared/ipc-types";

vi.mock("react-i18next", async () => (await import("./pm-test-i18n")).pmI18nMock);

const meter = (id: number, unit: number | null, name: string): PmAssetMeter => ({
  id,
  name,
  meter_type: "hours",
  unit_value_id: unit,
  unit: unit === 1 ? "h" : unit === 2 ? "km" : null,
  current_reading: 0,
  lifetime_reading: 0,
  rollover_value: null,
  last_read_at: null,
  is_primary: true,
});

// Pump 10 (source) has hour meter 7; pump 20 only a km meter.
const METERS: Record<number, PmAssetMeter[]> = {
  10: [meter(7, 1, "Heures marche")],
  20: [meter(8, 2, "Kilométrage")],
};

vi.mock("@/services/pm-service", () => ({
  listPmAssetMeters: vi.fn((id: number) => Promise.resolve(METERS[id] ?? [])),
  listPmScopeOptions: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({ onChange }: { onChange: (a: unknown) => void }) => (
    <button type="button" onClick={() => onChange({ id: 20, asset_code: "P-20" })}>
      pick pump 20
    </button>
  ),
}));

const TS = "2026-10-01T10:00:00Z";

function planOf(strategy: string): PmPlanRow {
  return {
    id: 1,
    code: "PM-PUMP",
    title: "Graissage pompe",
    description: null,
    asset_scope_type: "equipment",
    asset_scope_id: 10,
    strategy_type: strategy,
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
    lifecycle_status: "active",
    current_version_id: 5,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    asset_code: "P-10",
    asset_name: "Pompe 10",
  };
}

function versionOf(id: number, no: number, status: string, trigger: string): PmPlanVersion {
  return {
    id,
    pm_plan_id: 1,
    version_no: no,
    status,
    effective_from: TS,
    effective_to: null,
    trigger_definition_json: trigger,
    task_package_json: null,
    required_parts_json: null,
    required_skills_json: null,
    required_tools_json: null,
    estimated_duration_hours: null,
    planned_crew_size: null,
    estimated_labor_cost: null,
    estimated_parts_cost: null,
    estimated_service_cost: null,
    change_reason: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
  };
}

const METER_TRIGGER = '{"asset_meter_id":7,"interval_value":250}';

function renderDialog(strategy: string, trigger: string) {
  const onDuplicate = vi.fn().mockResolvedValue({});
  const onClose = vi.fn();
  render(
    <PmDuplicatePlanDialog
      open
      plan={planOf(strategy)}
      versions={[versionOf(5, 2, "published", trigger), versionOf(6, 3, "draft", "{}")]}
      onClose={onClose}
      onDuplicate={onDuplicate}
    />,
  );
  return { onDuplicate, onClose };
}

describe("PmDuplicatePlanDialog", () => {
  it("prefills the title, keeps the source pump and its own meter, and creates the copy", async () => {
    const { onDuplicate, onClose } = renderDialog("meter", METER_TRIGGER);
    expect(screen.getByLabelText("Title")).toHaveValue("Copy of Graissage pompe");
    expect(screen.getByText(/takes version v2/)).toBeInTheDocument();
    expect(screen.getByText(/Same equipment as the original plan: P-10/)).toBeInTheDocument();
    // The copy button waits for a code.
    const duplicate = screen.getByRole("button", { name: "Duplicate" });
    expect(duplicate).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Code"), { target: { value: "PM-PUMP-2" } });
    expect(await screen.findByRole("combobox")).toHaveTextContent("Heures marche (h)");
    fireEvent.click(duplicate);
    await waitFor(() => expect(onDuplicate).toHaveBeenCalled());
    expect(onDuplicate.mock.calls[0]?.[0]).toEqual({
      source_plan_id: 1,
      code: "PM-PUMP-2",
      title: "Copy of Graissage pompe",
      target_scope_type: "equipment",
      target_scope_id: 10,
      target_meter_id: 7,
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("on another pump without a meter of the same unit, leaves the meter to choose and says publishing waits", async () => {
    const { onDuplicate } = renderDialog("meter", METER_TRIGGER);
    fireEvent.click(screen.getByRole("button", { name: "pick pump 20" }));
    expect(await screen.findByText(/has no meter with the same unit/)).toBeInTheDocument();
    expect(screen.getByText(/cannot be published until a meter is chosen/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Code"), { target: { value: "pm-pump-20" } });
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    await waitFor(() => expect(onDuplicate).toHaveBeenCalled());
    expect(onDuplicate.mock.calls[0]?.[0]).toMatchObject({
      target_scope_id: 20,
      target_meter_id: null,
    });
  });

  it("asks no meter for a calendar plan", async () => {
    const { onDuplicate } = renderDialog("fixed", '{"interval_value":3,"interval_unit":"month"}');
    expect(screen.queryByText("Meter of the target equipment")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Code"), { target: { value: "PM-CAL" } });
    fireEvent.click(screen.getByRole("button", { name: "Duplicate" }));
    await waitFor(() => expect(onDuplicate).toHaveBeenCalled());
    expect(onDuplicate.mock.calls[0]?.[0]).toMatchObject({ target_meter_id: null });
  });
});

describe("what is copied", () => {
  it("takes the published version, else the latest draft", () => {
    const published = versionOf(5, 2, "published", "{}");
    const draft3 = versionOf(6, 3, "draft", "{}");
    const draft1 = versionOf(4, 1, "draft", "{}");
    expect(versionToCopy({ current_version_id: 5 }, [draft1, published, draft3])).toBe(published);
    expect(versionToCopy({ current_version_id: null }, [draft1, draft3])).toBe(draft3);
    expect(versionToCopy({ current_version_id: null }, [])).toBeNull();
  });

  it("finds the chosen meter of a meter plan and of a calendar plan's meter block", () => {
    expect(sourceMeterIdOf("meter", METER_TRIGGER)).toBe(7);
    expect(sourceMeterIdOf("fixed", '{"interval_value":1,"meter":{"asset_meter_id":9}}')).toBe(9);
    expect(sourceMeterIdOf("fixed", '{"interval_value":1}')).toBeNull();
    expect(sourceMeterIdOf("meter", '{"meter_type":"HOURS"}')).toBeNull();
    expect(sourceMeterIdOf("meter", "not json")).toBeNull();
  });
});

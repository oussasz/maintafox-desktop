import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enReliability from "@/i18n/locale-data/en/reliability.json";
import frReliability from "@/i18n/locale-data/fr/reliability.json";
import type { AssetCostHistory } from "@/services/finance-cost-service";
import type { EquipmentLife, RbdSpec } from "@/services/reliability-model-service";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enReliability,
    );
  return typeof value === "string" ? value : undefined;
}

const t = (key: string, opts?: Record<string, unknown>) =>
  (
    lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
const i18n = { language: "en", resolvedLanguage: "en" };
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t, i18n }),
}));

const permissions = { can: vi.fn<(p: string) => boolean>(), canAny: () => true };
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => permissions,
}));

const historyMock = vi.fn<(filter: unknown) => Promise<AssetCostHistory>>();
vi.mock("@/services/finance-cost-service", () => ({
  listAssetCostHistory: (filter: unknown) => historyMock(filter),
}));

import { AdvancedTools } from "../study/AdvancedTools";

const equipment: EquipmentLife[] = [
  {
    equipment_id: 7,
    asset_code: "PMP-001",
    asset_name: "Pump",
    failures: 3,
    hours: 4000,
    mtbf_hours: 1300,
    mttr_hours: 6,
  },
];
const rbd = { root_id: "root", nodes: [] } as RbdSpec;

function costRow(overrides: Record<string, unknown> = {}) {
  return {
    equipment_id: 7,
    equipment_code: "PMP-001",
    equipment_name: "Pump",
    cost_center_code: null,
    replacement_value: null,
    lifetime_cost: 0,
    lifetime_labor: 0,
    lifetime_parts: 0,
    lifetime_services: 0,
    cost_12m: 0,
    closed_work_orders: 5,
    corrective_work_orders: 4,
    corrective_cost: 4800,
    preventive_cost: 0,
    avg_corrective_cost: 1200,
    avg_preventive_cost: null,
    cost_12m_to_rav_pct: null,
    lifetime_to_rav_pct: null,
    last_closed_at: "2026-09-30T10:00:00Z",
    by_year: [],
    ...overrides,
  };
}

function history(rows: unknown[]): AssetCostHistory {
  return {
    currency_code: "EUR",
    rows,
    fleet_replacement_value: 0,
    fleet_cost_12m: 0,
    fleet_cost_to_rav_pct: null,
    assets_without_rav: 0,
  } as AssetCostHistory;
}

function openSimulation() {
  render(<AdvancedTools studyId={1} rbd={rbd} missionHours={8760} equipment={equipment} />);
  fireEvent.click(
    screen.getByRole("button", { name: enReliability.rams.advanced.simulation.title }),
  );
}

function repairCostInput(): HTMLInputElement {
  return screen.getByLabelText(
    enReliability.rams.advanced.simulation.repairCost,
  ) as HTMLInputElement;
}

const useMeanLabel = "Use the mean corrective cost of PMP-001";

describe("RAMS simulation: repair cost from Finance", () => {
  beforeEach(() => {
    historyMock.mockReset();
    permissions.can.mockReset().mockReturnValue(true);
  });

  it("has FR and EN text for every new line", () => {
    for (const bundle of [enReliability, frReliability]) {
      const s = bundle.rams.advanced.simulation;
      expect(s.useMeanCost).toBeTruthy();
      expect(s.meanCostSource).toBeTruthy();
      expect(s.meanCostPeriodTo).toBeTruthy();
      expect(s.meanCostPeriodAll).toBeTruthy();
      expect(s.meanCostNone).toBeTruthy();
      expect(s.meanCostError).toBeTruthy();
      expect(s.meanCostRestricted).toBeTruthy();
    }
  });

  it("leaves the field empty until the button is clicked, then fills an editable value with its source", async () => {
    historyMock.mockResolvedValue(history([costRow()]));
    openSimulation();
    expect(repairCostInput().value).toBe("");
    expect(historyMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: useMeanLabel }));
    await waitFor(() => expect(repairCostInput().value).toBe("1200"));
    expect(historyMock).toHaveBeenCalledWith(
      expect.objectContaining({ search: "PMP-001", only_with_cost: false }),
    );
    expect(screen.getByText(/mean of its 4 closed corrective work orders/)).toBeInTheDocument();
    expect(screen.getByText(/Currency: EUR/)).toBeInTheDocument();

    // The value stays editable, and a typed value no longer claims to come from Finance.
    fireEvent.change(repairCostInput(), { target: { value: "1500" } });
    expect(repairCostInput().value).toBe("1500");
    expect(screen.queryByText(/mean of its 4 closed corrective work orders/)).toBeNull();
  });

  it("fills nothing and says so when the equipment has no closed corrective work order", async () => {
    historyMock.mockResolvedValue(
      history([costRow({ corrective_work_orders: 0, avg_corrective_cost: null })]),
    );
    openSimulation();
    fireEvent.click(screen.getByRole("button", { name: useMeanLabel }));
    expect(await screen.findByText(/has no closed corrective work order yet/)).toBeInTheDocument();
    expect(repairCostInput().value).toBe("");
  });

  it("says so when Finance cannot be read, without filling the field", async () => {
    historyMock.mockRejectedValue(new Error("boom"));
    openSimulation();
    fireEvent.click(screen.getByRole("button", { name: useMeanLabel }));
    expect(await screen.findByText(/could not be read from Finance/)).toBeInTheDocument();
    expect(repairCostInput().value).toBe("");
  });

  it("offers no button without the finance view right and explains why", () => {
    permissions.can.mockReturnValue(false);
    openSimulation();
    expect(screen.queryByRole("button", { name: useMeanLabel })).toBeNull();
    expect(
      screen.getByText(enReliability.rams.advanced.simulation.meanCostRestricted),
    ).toBeInTheDocument();
    expect(historyMock).not.toHaveBeenCalled();
  });
});

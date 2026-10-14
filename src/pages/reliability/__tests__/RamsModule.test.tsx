import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { StudyDataStep } from "@/components/reliability/study/StudyDataStep";
import { StudyLifeStep } from "@/components/reliability/study/StudyLifeStep";
import enCommon from "@/i18n/en/common.json";
import enReliability from "@/i18n/locale-data/en/reliability.json";
import { RamsFleetPage } from "@/pages/reliability/RamsFleetPage";
import type { ReliabilityStudy, StudyDataset } from "@/services/reliability-study-service";
import { useLocaleStore } from "@/stores/locale-store";
import { mockInvoke } from "@/test/mocks/tauri";

// Keys resolve against the real English catalogue: a missing key fails the test.
function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}
const interpolate = (v: string, o?: Record<string, unknown>) =>
  v.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(o?.[n] ?? ""));

function translate(key: string, opts?: Record<string, unknown>): string {
  const count = opts?.["count"];
  const plural = typeof count === "number" ? `${key}_${count === 1 ? "one" : "other"}` : null;
  const value = (plural ? lookup(enReliability, plural) : undefined) ?? lookup(enReliability, key);
  if (value != null) return interpolate(value, opts);
  if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
  throw new Error(`missing reliability i18n key: ${key}`);
}
function translateCommon(key: string, opts?: Record<string, unknown>): string {
  const value = lookup(enCommon, key);
  return value != null ? interpolate(value, opts) : key;
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      ns === "reliability" ? translate(key, opts) : translateCommon(key, opts),
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const TS = "2026-10-01T10:00:00Z";

const fleetView = {
  period_start: "2024-10-01T00:00:00Z",
  period_end: TS,
  kpis: {
    equipment_count: 2,
    failure_count: 10,
    downtime_hours: 244,
    availability_operational: 0.964,
    mtbf_hours: 2310,
    mttr_hours: 6.8,
    data_quality: 0.82,
    records_missing_mode: 1,
    records_missing_cause: 1,
    operating_basis_count: 2,
    calendar_basis_count: 0,
  },
  rows: [
    {
      equipment_id: 10,
      asset_code: "P-101",
      asset_name: "Transfer pump 1",
      class_id: 3,
      class_name: "Centrifugal pump",
      failures: 6,
      downtime_hours: 148,
      downtime_share: 0.6066,
      mtbf_hours: 1820,
      mttr_hours: 7.4,
      time_basis: "run_hours_meter",
      data_quality: "to_check",
      records_to_check: 2,
      last_study_id: null,
      last_study_title: null,
      last_study_updated_at: null,
    },
    {
      equipment_id: 11,
      asset_code: "P-102",
      asset_name: "Transfer pump 2",
      class_id: 3,
      class_name: "Centrifugal pump",
      failures: 4,
      downtime_hours: 96,
      downtime_share: 0.3934,
      mtbf_hours: 2410,
      mttr_hours: 8.1,
      time_basis: "run_hours_meter",
      data_quality: "complete",
      records_to_check: 0,
      last_study_id: null,
      last_study_title: null,
      last_study_updated_at: null,
    },
  ],
  total_rows: 2,
  org_nodes: [{ id: 1, label: "Main plant", depth: 0 }],
  equipment_classes: [{ id: 3, label: "Centrifugal pump", depth: 0 }],
};

const study: ReliabilityStudy = {
  id: 7,
  title: "P-101, P-102 · Bearing seizure",
  period_start: "2024-10-01T00:00:00Z",
  period_end: TS,
  failure_mode_id: 5,
  failure_mode_code: "BRD",
  failure_mode_label: "Bearing seizure",
  time_basis: "auto",
  planned_cost: null,
  failure_cost: null,
  cost_currency_code: null,
  pm_plan_id: null,
  competing_modes: "ignore",
  mission_hours: null,
  status: "active",
  assets: [
    {
      equipment_id: 10,
      asset_code: "P-101",
      asset_name: "Transfer pump 1",
      class_id: 3,
      class_name: "Centrifugal pump",
    },
    {
      equipment_id: 11,
      asset_code: "P-102",
      asset_name: "Transfer pump 2",
      class_id: 3,
      class_name: "Centrifugal pump",
    },
  ],
  decision_count: 0,
  created_by_name: "Admin",
  row_version: 2,
  created_at: TS,
  updated_at: TS,
};

beforeEach(() => {
  mockInvoke.mockReset();
  useLocaleStore.setState({ activeLocale: "en", activeCurrencyCode: "DZD" });
});

describe("RamsFleetPage", () => {
  it("shows fleet KPIs and opens a study for the selected equipment", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_fleet_view") return fleetView;
      if (cmd === "list_reliability_studies") return [];
      if (cmd === "create_reliability_study") return study;
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <MemoryRouter initialEntries={["/reliability"]}>
        <Routes>
          <Route path="/reliability" element={<RamsFleetPage />} />
          <Route path="/reliability/studies/:id" element={<p>study opened</p>} />
        </Routes>
      </MemoryRouter>,
    );
    expect(await screen.findByText("P-101")).toBeInTheDocument();
    expect(screen.getByText("96.4")).toBeInTheDocument();
    expect(screen.getByText("2,310")).toBeInTheDocument();
    expect(screen.getByText("2 to check")).toBeInTheDocument();
    const start = screen.getByRole("button", { name: /Study the selection/ });
    expect(start).toBeDisabled();

    fireEvent.click(screen.getByRole("checkbox", { name: "Select P-101" }));
    const row = screen.getByText("P-102").closest("tr");
    if (!row) throw new Error("P-102 row missing");
    fireEvent.keyDown(row, { key: " " });
    const go = screen.getByRole("button", { name: /Study 2 equipment/ });
    fireEvent.click(go);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("create_reliability_study", {
        input: expect.objectContaining({ equipment_ids: [10, 11] }),
      }),
    );
    expect(await screen.findByText("study opened")).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("get_reliability_fleet_view", {
      filter: expect.objectContaining({ org_node_id: null, equipment_class_id: null, limit: 50 }),
    });
  });

  it("explains an empty period instead of showing an empty table", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_reliability_fleet_view")
        return {
          ...fleetView,
          rows: [],
          total_rows: 0,
          kpis: { ...fleetView.kpis, failure_count: 0, data_quality: null },
        };
      if (cmd === "list_reliability_studies") return [];
      throw new Error(`unexpected ${cmd}`);
    });
    render(
      <MemoryRouter>
        <RamsFleetPage />
      </MemoryRouter>,
    );
    expect(await screen.findByText("No failures in the period")).toBeInTheDocument();
  });
});

const dataset: StudyDataset = {
  study,
  effective_time_basis: "operating",
  clocks: [],
  records: [
    {
      key: "failure:1",
      kind: "failure",
      equipment_id: 10,
      asset_code: "P-101",
      failure_event_id: 1,
      work_order_id: 41,
      work_order_code: "OT-2025-0412",
      di_id: 9,
      di_code: "DI-2025-0388",
      event_at: "2025-08-02T10:00:00Z",
      event_time_source: "repair_start",
      restored_at: "2025-08-03T10:00:00Z",
      age_hours: 3620,
      included: true,
      state_reason: null,
      analyst_note: null,
      failure_mode_code: "BRD",
      failure_mode_label: "Bearing seizure",
      failure_cause_label: "Contamination",
      failure_mechanism_label: "Wear",
      cause_not_determined: false,
      downtime_hours: 24,
      flags: ["di_earlier"],
      di_at: "2025-08-01T03:00:00Z",
      di_lead_hours: 31,
    },
    {
      key: "pm:52",
      kind: "pm_suspension",
      equipment_id: 11,
      asset_code: "P-102",
      failure_event_id: null,
      work_order_id: 52,
      work_order_code: "OT-2025-0310",
      di_id: null,
      di_code: null,
      event_at: "2025-06-11T10:00:00Z",
      event_time_source: "pm_completion",
      restored_at: null,
      age_hours: 2200,
      included: false,
      state_reason: "pm_not_confirmed",
      analyst_note: null,
      failure_mode_code: null,
      failure_mode_label: null,
      failure_cause_label: null,
      failure_mechanism_label: null,
      cause_not_determined: false,
      downtime_hours: null,
      flags: [],
      di_at: null,
      di_lead_hours: null,
    },
  ],
  checks: [
    {
      kind: "di_earlier",
      severity: "warning",
      record_key: "failure:1",
      equipment_id: 10,
      asset_code: "P-101",
      work_order_id: 41,
      work_order_code: "OT-2025-0412",
      di_code: "DI-2025-0388",
      hours: 31,
      count: null,
      coverage: null,
    },
    {
      kind: "operating_hours",
      severity: "success",
      record_key: null,
      equipment_id: 10,
      asset_code: "P-101",
      work_order_id: null,
      work_order_code: null,
      di_code: null,
      hours: null,
      count: null,
      coverage: 0.98,
    },
  ],
  failures_used: 1,
  suspensions_used: 0,
  excluded: 1,
  history: [],
};

describe("StudyDataStep", () => {
  it("excludes a record only with a reason and applies the request time", async () => {
    mockInvoke.mockResolvedValue(dataset);
    const changed = vi.fn();
    render(
      <MemoryRouter>
        <StudyDataStep dataset={dataset} editable onChanged={changed} onNext={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.getByText("OT-2025-0412: failure time = repair start")).toBeInTheDocument();
    expect(screen.getByText("Request DI-2025-0388 was raised 31 h earlier.")).toBeInTheDocument();
    expect(screen.getByText("Readings cover 98 % of the period.")).toBeInTheDocument();
    expect(
      screen.getByText("To confirm: did this preventive work replace the part?"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("checkbox", { name: "Use OT-2025-0412 in the analysis" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Exclude the record" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "Assembly defect" } });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("set_reliability_study_record", {
        input: {
          study_id: 7,
          record_key: "failure:1",
          action: "exclude",
          reason: "Assembly defect",
        },
      }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Use the request time" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("set_reliability_study_record", {
        input: { study_id: 7, record_key: "failure:1", action: "use_di_time", reason: null },
      }),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Use OT-2025-0310 in the analysis" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("set_reliability_study_record", {
        input: { study_id: 7, record_key: "pm:52", action: "include", reason: null },
      }),
    );
    expect(changed).toHaveBeenCalled();
  });
});

const fitAnalysis = {
  effective_time_basis: "operating",
  n_failures: 8,
  n_suspensions: 2,
  min_failures: 5,
  fit: {
    beta: 3.04,
    eta: 4440,
    confidence: 0.9,
    beta_lower: 1.9,
    beta_upper: 4.8,
    eta_lower: 3700,
    eta_upper: 5300,
    b10: 2118,
    b10_lower: 1400,
    b10_upper: 3000,
    mean_life: 3967,
    median_life: 3940,
    rho: 0.9996,
    pattern: "wear_out",
  },
  fit_issue: null,
  plot_points: [
    { time: 1850, median_rank: 0.067 },
    { time: 2610, median_rank: 0.163 },
  ],
  band: [
    { time: 1000, unreliability: 0.02, lower: 0.005, upper: 0.06 },
    { time: 5000, unreliability: 0.7, lower: 0.5, upper: 0.85 },
  ],
  curve: [
    { time: 1000, reliability: 0.99, hazard: 0.00002 },
    { time: 8000, reliability: 0.01, hazard: 0.002 },
  ],
  cost: {
    planned_cost: 180000,
    failure_cost: 1450000,
    run_to_failure: { interval: null, cost_rate: 365.5, annual_cost: 5848000 },
    current: { interval: 6000, cost_rate: 343.8, annual_cost: 5500800 },
    proposed: { interval: 1852, cost_rate: 146.1, annual_cost: 2337600 },
    rejection: null,
    saving_vs_run_to_failure: 0.6,
    annual_saving_vs_current: 3163200,
    curve: [
      { interval: 500, cost_rate: 400 },
      { interval: 1852, cost_rate: 146.1 },
      { interval: 6000, cost_rate: 343.8 },
    ],
  },
  cost_source: "class_default",
  cost_currency_code: "DZD",
  cost_class_id: 3,
  cost_class_name: "Centrifugal pump",
  pm_plans: [
    {
      pm_plan_id: 14,
      code: "PM-PMP-014",
      title: "Pump bearing",
      equipment_id: 10,
      asset_code: "P-101",
      strategy_type: "meter",
      interval_value: 6000,
      interval_unit: "operating_hours",
      interval_hours: 6000,
      interval_note: null,
    },
  ],
  selected_pm_plan_id: 14,
  operating_hours_per_year: 16000,
  asset_count: 2,
  operating_ratio: 0.91,
};

describe("StudyLifeStep", () => {
  it("reads the fit, compares with the PM plan and proposes the optimal interval", async () => {
    mockInvoke.mockResolvedValue(fitAnalysis);
    const decide = vi.fn();
    render(
      <StudyLifeStep
        study={study}
        editable
        datasetVersion="v1"
        onStudyChanged={vi.fn()}
        onDecide={decide}
      />,
    );
    expect(await screen.findByText("3.04")).toBeInTheDocument();
    expect(screen.getByText("90 %: 1.90 – 4.80")).toBeInTheDocument();
    expect(screen.getByText("Wear-out (β > 1)")).toBeInTheDocument();
    expect(
      screen.getByText(/Plan PM-PMP-014 replaces them at 6,000 h, after half of the failures./),
    ).toBeInTheDocument();
    expect(screen.getByText("Defaults of class Centrifugal pump.")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Weibull probability plot" })).toBeInTheDocument();
    expect(screen.getByText("Proposed: replacement at 1,900 h")).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("get_reliability_study_life_analysis", {
      input: { study_id: 7, planned_cost: null, failure_cost: null, pm_plan_id: null },
    });
    fireEvent.click(screen.getByRole("button", { name: "Record this decision" }));
    expect(decide).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "pm_interval", pmPlanId: 14, intervalHours: 1900 }),
    );
  });

  it("explains why no fit is shown when failures are too few", async () => {
    mockInvoke.mockResolvedValue({
      ...fitAnalysis,
      n_failures: 3,
      fit: null,
      fit_issue: "not_enough_failures",
      cost: null,
      band: [],
      curve: [],
    });
    render(
      <StudyLifeStep
        study={study}
        editable
        datasetVersion="v1"
        onStudyChanged={vi.fn()}
        onDecide={vi.fn()}
      />,
    );
    expect(await screen.findByText("Not enough failures")).toBeInTheDocument();
    expect(screen.getByText(/at least 5 failures in use \(currently 3\)/)).toBeInTheDocument();
  });
});

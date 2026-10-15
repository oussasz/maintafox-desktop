/**
 * Finance components: OT cost panel (estimated / planned / committed / actual, pending
 * settlement reason), settlement worklist (retry), period close (blockers), budget cockpit
 * (figures, drill-down, empty setup state) and the pure helpers behind them.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { BudgetCockpitView, cockpitCsv } from "@/components/finance/BudgetCockpitView";
import { BudgetPeriodsPanel, closeBlocker } from "@/components/finance/BudgetPeriodsPanel";
import { SettlementWorklist } from "@/components/finance/SettlementWorklist";
import { variancePct } from "@/components/finance/VarianceReviewsPanel";
import { parseAmount } from "@/components/finance/WoCostEstimateDialog";
import { WoCostPanel, costVariance } from "@/components/finance/WoCostPanel";
import { forecastTone } from "@/components/finance/finance-tone";
import {
  catalogLabel,
  periodCalendarMonth,
  resetFinanceCatalogCache,
} from "@/components/finance/use-finance-catalogs";
import { useFormatters } from "@/hooks/use-formatters";
import enCommon from "@/i18n/en/common.json";
import enBudget from "@/i18n/locale-data/en/budget.json";
import type {
  BudgetCockpit,
  BudgetPeriod,
  WoCostSettlement,
  WoCostView,
} from "@/services/finance-cost-service";
import { P } from "@shared/rbac/permissions.generated";

const svc = vi.hoisted(() => ({
  listFinanceCatalogs: vi.fn(),
  getWoCostView: vi.fn(),
  upsertWoCostEstimate: vi.fn(),
  listWoSettlements: vi.fn(),
  retryWoSettlement: vi.fn(),
  getBudgetCockpit: vi.fn(),
  getBudgetCockpitDrilldown: vi.fn(),
  listBudgetPeriods: vi.fn(),
  closeBudgetPeriod: vi.fn(),
  reopenBudgetPeriod: vi.fn(),
  listAssetCostHistory: vi.fn(),
}));
vi.mock("@/services/finance-cost-service", () => svc);

const perms = vi.hoisted(() => ({ granted: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => perms.granted.has(p),
    canAny: (...p: string[]) => p.some((x) => perms.granted.has(x)),
    canAll: (...p: string[]) => p.every((x) => perms.granted.has(x)),
    permissions: [],
    isLoading: false,
    refresh: async () => undefined,
  }),
}));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: null,
  }),
}));

function money(value: number, currency?: string): string {
  let out = "";
  function Probe() {
    out = useFormatters().formatCurrency(value, currency);
    return null;
  }
  render(<Probe />);
  return out;
}

const settlement = (over: Partial<WoCostSettlement> = {}): WoCostSettlement => ({
  id: 5,
  entity_sync_id: "s-5",
  work_order_id: 42,
  work_order_code: "OT-0042",
  work_order_title: "Fuite garniture pompe",
  equipment_id: 1,
  equipment_code: "PMP-001",
  settlement_cycle: 0,
  status: "pending",
  pending_reason: "no_cost_center",
  cost_center_id: null,
  cost_center_code: null,
  cost_center_name: null,
  budget_version_id: null,
  budget_version_title: null,
  fiscal_year: 2026,
  period_month: null,
  currency_code: "DZD",
  labor_amount: 100,
  labor_overtime_amount: 0,
  parts_amount: 10,
  services_amount: 300,
  other_amount: 50,
  total_amount: 460,
  overtime_captured: false,
  unpriced_labor_hours: 0,
  unvalued_parts_qty: 0,
  closed_at: "2026-10-10T10:00:00Z",
  posted_at: null,
  reversed_at: null,
  reversal_reason: null,
  row_version: 1,
  ...over,
});

const figures = (
  estimated: number | null,
  planned: number | null,
  committed: number,
  actual: number,
) => ({ estimated, planned, committed, actual });

const costView = (
  over: Partial<WoCostView["breakdown"]> = {},
  settlements: WoCostSettlement[] = [],
): WoCostView => ({
  breakdown: {
    work_order_id: 42,
    work_order_code: "OT-0042",
    status_code: "in_progress",
    equipment_id: 1,
    currency_code: "DZD",
    labor: figures(80, 100, 0, 100),
    parts: figures(20, null, 80, 10),
    services: figures(null, 300, 0, 300),
    other: figures(null, null, 0, 50),
    total: figures(100, 400, 80, 460),
    labor_overtime_amount: 0,
    overtime_captured: false,
    labor_hours: 2,
    unpriced_labor_hours: 1.5,
    unvalued_parts_qty: 0,
    planned_labor_gap: null,
    labor_lines: [
      {
        intervener_entry_id: 1,
        personnel_id: 3,
        person_name: "Karim Benali",
        skill_label: "Mécanicien",
        hours: 2,
        hourly_rate: 50,
        is_overtime: false,
        amount: 100,
      },
    ],
    parts_lines: [],
    purchase_lines: [],
    estimate: null,
    cost_center: null,
    ...over,
  },
  settlements,
});

const catalogs = {
  cost_categories: [
    {
      code: "labor",
      label: "Main-d'œuvre",
      description: "Labour",
      is_active: true,
      is_system: true,
      sort_order: 1,
    },
    {
      code: "parts",
      label: "Pièces",
      description: "Parts",
      is_active: true,
      is_system: true,
      sort_order: 2,
    },
    {
      code: "services",
      label: "Sous-traitance et services",
      description: "Contractors and services",
      is_active: true,
      is_system: true,
      sort_order: 3,
    },
    {
      code: "other",
      label: "Autres coûts",
      description: "Other costs",
      is_active: true,
      is_system: true,
      sort_order: 4,
    },
  ],
  variance_reasons: [
    {
      code: "labor_overrun",
      label: "Dépassement d'heures",
      description: "Labour overrun",
      is_active: true,
      is_system: true,
      sort_order: 1,
    },
  ],
  fiscal_year_start_month: 1,
};

beforeAll(async () => {
  Element.prototype.scrollIntoView = vi.fn();
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "en",
      fallbackLng: "en",
      ns: ["budget", "common"],
      defaultNS: "budget",
      resources: { en: { budget: enBudget, common: enCommon } },
      interpolation: { escapeValue: false },
    });
  } else {
    i18next.addResourceBundle("en", "budget", enBudget, true, true);
  }
});

beforeEach(() => {
  for (const fn of Object.values(svc)) fn.mockReset();
  resetFinanceCatalogCache();
  svc.listFinanceCatalogs.mockResolvedValue(catalogs);
  perms.granted = new Set();
});

describe("finance helpers", () => {
  it("maps fiscal periods to calendar months from the start month", () => {
    expect(periodCalendarMonth(1, 1)).toBe(1);
    expect(periodCalendarMonth(1, 7)).toBe(7);
    expect(periodCalendarMonth(6, 7)).toBe(12);
    expect(periodCalendarMonth(7, 7)).toBe(1);
    expect(periodCalendarMonth(12, 7)).toBe(6);
  });

  it("labels catalog values in the interface language, never inventing one", () => {
    const value = catalogs.cost_categories[0] as (typeof catalogs.cost_categories)[number];
    expect(catalogLabel(value, "fr")).toBe("Main-d'œuvre");
    expect(catalogLabel(value, "en")).toBe("Labour");
    expect(catalogLabel({ ...value, description: null }, "en")).toBe("Main-d'œuvre");
  });

  it("measures the OT variance against the estimate, else the plan", () => {
    expect(costVariance(figures(80, 100, 0, 100))).toEqual({ base: "estimated", amount: 20 });
    expect(costVariance(figures(null, 300, 0, 250))).toEqual({ base: "planned", amount: -50 });
    expect(costVariance(figures(null, null, 0, 50))).toBeNull();
  });

  it("parses typed amounts with comma or dot and refuses negatives", () => {
    expect(parseAmount("1 250,50")).toBe(1250.5);
    expect(parseAmount("")).toBe(0);
    expect(parseAmount("-3")).toBeNull();
    expect(parseAmount("abc")).toBeNull();
  });

  it("colours a forecast by its budget", () => {
    expect(forecastTone(1000, 1100)).toBe("danger");
    expect(forecastTone(1000, 950)).toBe("warning");
    expect(forecastTone(1000, 500)).toBeUndefined();
    expect(forecastTone(0, 10)).toBe("attention");
  });

  it("states the variance as a signed share of budget", () => {
    expect(variancePct({ variance: -200, budget: 1000 })).toBe(-20);
    expect(variancePct({ variance: -200, budget: 0 })).toBe(0);
  });

  it("blocks closing a running month or one with pending OTs", () => {
    const p = (over: Partial<BudgetPeriod>): BudgetPeriod => ({
      fiscal_year: 2026,
      period_month: 9,
      status: "open",
      closed_at: null,
      closed_by_name: null,
      reopened_at: null,
      notes: null,
      actual_amount: 0,
      pending_settlements: 0,
      is_future: false,
      calendar_year: 2026,
      calendar_month: 9,
      row_version: null,
      ...over,
    });
    expect(closeBlocker(p({}))).toBeNull();
    expect(closeBlocker(p({ is_future: true }))).toBe("future");
    expect(closeBlocker(p({ pending_settlements: 2 }))).toBe("pending");
    expect(closeBlocker(p({ status: "closed" }))).toBe("closed");
  });
});

describe("WoCostPanel", () => {
  it("shows estimated, planned, committed and actual by category with the notices", async () => {
    perms.granted = new Set([P.OT_EDIT]);
    svc.getWoCostView.mockResolvedValue(costView());
    render(
      <MemoryRouter>
        <WoCostPanel woId={42} status="in_progress" />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Work order costs")).toBeInTheDocument();
    expect(svc.getWoCostView).toHaveBeenCalledWith(42);
    const text = document.body.textContent ?? "";
    expect(text).toContain(money(460, "DZD"));
    expect(text).toContain(money(80, "DZD"));
    expect(text).toContain("h recorded without an hourly rate");
    expect(text).toContain("Without a cost centre the settlement will stay pending");
    expect(text).toContain("Overtime is not yet distinguished");
    expect(screen.getByRole("button", { name: /Estimate/ })).toBeInTheDocument();
  });

  it("explains a pending settlement and hides the estimate on a closed OT", async () => {
    perms.granted = new Set([P.OT_EDIT]);
    svc.getWoCostView.mockResolvedValue(
      costView({ status_code: "closed", unpriced_labor_hours: 0 }, [settlement()]),
    );
    render(
      <MemoryRouter>
        <WoCostPanel woId={42} status="closed" />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Settlement pending")).toBeInTheDocument();
    expect(document.body.textContent).toContain("Set the cost centre on the asset");
    expect(screen.queryByRole("button", { name: /Estimate/ })).not.toBeInTheDocument();
  });
});

describe("SettlementWorklist", () => {
  it("lists pending settlements with the fix and retries them", async () => {
    perms.granted = new Set([P.FIN_POST]);
    svc.listWoSettlements.mockResolvedValue([settlement()]);
    svc.retryWoSettlement.mockResolvedValue(
      settlement({
        status: "posted",
        pending_reason: null,
        cost_center_code: "CC-1",
        period_month: 10,
      }),
    );
    const onChanged = vi.fn();
    render(
      <MemoryRouter>
        <SettlementWorklist fiscalYear={2026} onChanged={onChanged} />
      </MemoryRouter>,
    );
    expect(await screen.findByText("No cost centre for this work order.")).toBeInTheDocument();
    expect(svc.listWoSettlements).toHaveBeenCalledWith({
      fiscal_year: 2026,
      status: "pending",
      limit: 1000,
    });
    fireEvent.click(screen.getByRole("button", { name: "Settle" }));
    await waitFor(() => expect(svc.retryWoSettlement).toHaveBeenCalledWith(5));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("offers no retry without the posting right", async () => {
    svc.listWoSettlements.mockResolvedValue([settlement()]);
    render(
      <MemoryRouter>
        <SettlementWorklist fiscalYear={2026} />
      </MemoryRouter>,
    );
    await screen.findByText("No cost centre for this work order.");
    expect(screen.queryByRole("button", { name: "Settle" })).not.toBeInTheDocument();
  });
});

describe("BudgetPeriodsPanel", () => {
  it("closes an ended month and keeps a month with pending OTs locked", async () => {
    perms.granted = new Set([P.FIN_POST]);
    const months: BudgetPeriod[] = Array.from({ length: 12 }, (_, i) => ({
      fiscal_year: 2025,
      period_month: i + 1,
      status: "open",
      closed_at: null,
      closed_by_name: null,
      reopened_at: null,
      notes: null,
      actual_amount: 0,
      pending_settlements: i === 1 ? 1 : 0,
      is_future: false,
      calendar_year: 2025,
      calendar_month: i + 1,
      row_version: null,
    }));
    svc.listBudgetPeriods.mockResolvedValue(months);
    svc.closeBudgetPeriod.mockResolvedValue(months);
    render(<BudgetPeriodsPanel fiscalYear={2025} currency="DZD" />);
    const buttons = await screen.findAllByRole("button", { name: "Close" });
    expect(buttons).toHaveLength(12);
    expect(buttons[1]).toBeDisabled();
    fireEvent.click(buttons[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    const confirm = Array.from(dialog.querySelectorAll("button")).find(
      (b) => b.textContent === "Close",
    );
    fireEvent.click(confirm as HTMLElement);
    await waitFor(() =>
      expect(svc.closeBudgetPeriod).toHaveBeenCalledWith({
        fiscal_year: 2025,
        period_month: 1,
        notes: null,
      }),
    );
  });
});

const cockpit = (over: Partial<BudgetCockpit> = {}): BudgetCockpit => ({
  fiscal_year: 2026,
  version: {
    id: 7,
    title: "Budget 2026",
    status: "approved",
    scenario_type: "approved",
    version_no: 1,
    currency_code: "DZD",
  },
  currency_code: "DZD",
  as_of_month: 10,
  fiscal_start_month: 1,
  totals: { budget: 4200, committed: 80, actual: 460, forecast: 1500, variance: 2700 },
  rows: [
    {
      cost_center_id: 3,
      cost_center_code: "CC-MNT-PMP",
      cost_center_name: "Maintenance pompes",
      bucket: "services",
      budget: 200,
      committed: 0,
      actual: 300,
      forecast: 300,
      variance: -100,
      consumed_pct: 150,
      open_reviews: 0,
      latest_driver_code: null,
    },
  ],
  months: Array.from({ length: 12 }, (_, i) => ({
    period_month: i + 1,
    calendar_month: i + 1,
    budget: 350,
    actual: i === 9 ? 460 : 0,
    cumulative_budget: 350 * (i + 1),
    cumulative_actual: i >= 9 ? 460 : 0,
  })),
  pending_settlements: 1,
  pending_settlement_amount: 120,
  unassigned_commitments: 0,
  setup: { cost_centers: 1, versions_for_year: 1, budget_lines: 3 },
  ...over,
});

describe("BudgetCockpitView", () => {
  it("shows the year figures, the pending warning and drills into a line", async () => {
    perms.granted = new Set([P.FIN_BUDGET]);
    svc.getBudgetCockpit.mockResolvedValue(cockpit());
    svc.getBudgetCockpitDrilldown.mockResolvedValue({
      actuals: [
        {
          actual_id: 1,
          period_month: 10,
          bucket: "services",
          amount: 300,
          posting_status: "posted",
          source_type: "wo_settlement",
          source_id: "s:services",
          rate_card_lane: null,
          work_order_id: 42,
          work_order_code: "OT-0042",
          work_order_title: "Fuite garniture",
          equipment_code: "PMP-001",
          event_at: "2026-10-10T10:00:00Z",
          is_reversal: false,
        },
      ],
      commitments: [],
    });
    const onNavigate = vi.fn();
    const onAnalyse = vi.fn();
    render(
      <MemoryRouter>
        <BudgetCockpitView
          fiscalYear={2026}
          budgetVersionId={7}
          onNavigate={onNavigate}
          onAnalyseVariance={onAnalyse}
        />
      </MemoryRouter>,
    );
    expect(await screen.findByText("CC-MNT-PMP")).toBeInTheDocument();
    expect(document.body.textContent).toContain(money(4200, "DZD"));
    expect(document.body.textContent).toContain("1 closed work order is waiting for settlement");
    fireEvent.click(screen.getByRole("button", { name: "Show pending settlements" }));
    expect(onNavigate).toHaveBeenCalledWith("settlements");
    fireEvent.click(screen.getByRole("button", { name: "Analyse variance" }));
    expect(onAnalyse).toHaveBeenCalled();
    fireEvent.click(screen.getByText("CC-MNT-PMP"));
    await waitFor(() =>
      expect(svc.getBudgetCockpitDrilldown).toHaveBeenCalledWith({
        fiscal_year: 2026,
        budget_version_id: 7,
        cost_center_id: 3,
        bucket: "services",
      }),
    );
    expect(await screen.findByText("1 actual entry")).toBeInTheDocument();
  });

  it("explains what to set up when there is no cost centre", async () => {
    perms.granted = new Set([P.FIN_BUDGET]);
    svc.getBudgetCockpit.mockResolvedValue(
      cockpit({ setup: { cost_centers: 0, versions_for_year: 0, budget_lines: 0 }, rows: [] }),
    );
    const onNavigate = vi.fn();
    render(
      <MemoryRouter>
        <BudgetCockpitView
          fiscalYear={2026}
          budgetVersionId={null}
          onNavigate={onNavigate}
          onAnalyseVariance={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(await screen.findByText("No cost centre")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open the budget" }));
    expect(onNavigate).toHaveBeenCalledWith("planning");
  });

  it("exports the rows as semicolon CSV", () => {
    const csv = cockpitCsv(cockpit(), ["cc", "name", "cat", "b", "c", "a", "f", "v"], (b) =>
      b.toUpperCase(),
    );
    expect(csv.split("\n")[1]).toBe(
      "CC-MNT-PMP;Maintenance pompes;SERVICES;200.00;0.00;300.00;300.00;-100.00",
    );
  });
});

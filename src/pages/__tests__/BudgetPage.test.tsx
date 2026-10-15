/**
 * Budget page: the reference version of the year feeds the cockpit, tabs open from the URL, and
 * the planning tab follows the version lifecycle and the budget rights.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enBudget from "@/i18n/locale-data/en/budget.json";
import { BudgetPage, defaultVersion } from "@/pages/BudgetPage";
import type { BudgetVersion } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const budget = vi.hoisted(() => ({
  listBudgetVersions: vi.fn(),
  listCostCenters: vi.fn(),
  listBudgetLines: vi.fn(),
  createCostCenter: vi.fn(),
  updateCostCenter: vi.fn(),
  createBudgetVersion: vi.fn(),
  createBudgetSuccessorVersion: vi.fn(),
  transitionBudgetVersionLifecycle: vi.fn(),
  createBudgetLine: vi.fn(),
  updateBudgetLine: vi.fn(),
  listBudgetVarianceReviews: vi.fn(),
  createBudgetVarianceReview: vi.fn(),
  transitionBudgetVarianceReview: vi.fn(),
}));
vi.mock("@/services/budget-service", () => budget);

const finance = vi.hoisted(() => ({
  listFinanceCatalogs: vi.fn(),
  getBudgetCockpit: vi.fn(),
  getBudgetCockpitDrilldown: vi.fn(),
  listWoSettlements: vi.fn(),
  retryWoSettlement: vi.fn(),
  listBudgetPeriods: vi.fn(),
  closeBudgetPeriod: vi.fn(),
  reopenBudgetPeriod: vi.fn(),
  listAssetCostHistory: vi.fn(),
}));
vi.mock("@/services/finance-cost-service", () => finance);

vi.mock("@/services/org-node-service", () => ({ listOrgTree: vi.fn().mockResolvedValue([]) }));

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

const year = new Date().getFullYear();

const version = (over: Partial<BudgetVersion>): BudgetVersion => ({
  id: 1,
  entity_sync_id: "v",
  fiscal_year: year,
  scenario_type: "original",
  version_no: 1,
  status: "draft",
  currency_code: "DZD",
  title: "Budget maintenance",
  planning_basis: null,
  source_basis_mix_json: null,
  labor_assumptions_json: null,
  baseline_reference: null,
  erp_external_ref: null,
  successor_of_version_id: null,
  created_by_id: 1,
  approved_at: null,
  approved_by_id: null,
  frozen_at: null,
  frozen_by_id: null,
  row_version: 1,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  ...over,
});

function renderPage(path = "/budget") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <BudgetPage />
    </MemoryRouter>,
  );
}

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
  for (const fn of [...Object.values(budget), ...Object.values(finance)]) fn.mockReset();
  finance.listFinanceCatalogs.mockResolvedValue(catalogs);
  perms.granted = new Set([P.FIN_VIEW]);
  budget.listCostCenters.mockResolvedValue([]);
  budget.listBudgetLines.mockResolvedValue([]);
  budget.listBudgetVarianceReviews.mockResolvedValue([]);
  finance.getBudgetCockpit.mockResolvedValue({
    fiscal_year: year,
    version: null,
    currency_code: "DZD",
    as_of_month: 1,
    fiscal_start_month: 1,
    totals: { budget: 0, committed: 0, actual: 0, forecast: 0, variance: 0 },
    rows: [],
    months: [],
    pending_settlements: 0,
    pending_settlement_amount: 0,
    unassigned_commitments: 0,
    setup: { cost_centers: 1, versions_for_year: 2, budget_lines: 0 },
  });
});

describe("defaultVersion", () => {
  it("prefers the approved posting version, else the latest", () => {
    const draft = version({ id: 1, version_no: 3 });
    const approved = version({ id: 2, status: "approved", scenario_type: "approved" });
    const whatIf = version({ id: 3, status: "approved", scenario_type: "what_if", version_no: 9 });
    expect(defaultVersion([draft, approved, whatIf])?.id).toBe(2);
    expect(defaultVersion([draft, version({ id: 4, version_no: 1 })])?.id).toBe(1);
    expect(defaultVersion([])).toBeNull();
  });
});

describe("BudgetPage", () => {
  it("feeds the cockpit with the year's approved version", async () => {
    budget.listBudgetVersions.mockResolvedValue([
      version({ id: 1, version_no: 2 }),
      version({ id: 7, status: "approved", scenario_type: "approved" }),
    ]);
    renderPage();
    expect(await screen.findByText("Budget and maintenance costs")).toBeInTheDocument();
    await waitFor(() =>
      expect(finance.getBudgetCockpit).toHaveBeenLastCalledWith({
        fiscal_year: year,
        budget_version_id: 7,
      }),
    );
    expect(screen.getByRole("tab", { name: "WO settlements" })).toBeInTheDocument();
  });

  it("opens the tab named in the URL", async () => {
    budget.listBudgetVersions.mockResolvedValue([]);
    finance.listWoSettlements.mockResolvedValue([]);
    renderPage("/budget?tab=settlements");
    expect(await screen.findByText("No pending settlement")).toBeInTheDocument();
  });

  it("keeps the budget read-only without the budget right", async () => {
    budget.listBudgetVersions.mockResolvedValue([version({ id: 1 })]);
    renderPage("/budget?tab=planning");
    expect(await screen.findByText(`${year} budget versions`)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Submit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New version" })).not.toBeInTheDocument();
  });

  it("moves a draft version through its lifecycle", async () => {
    perms.granted = new Set([P.FIN_VIEW, P.FIN_BUDGET]);
    budget.listBudgetVersions.mockResolvedValue([version({ id: 1 })]);
    budget.transitionBudgetVersionLifecycle.mockResolvedValue(
      version({ id: 1, status: "submitted" }),
    );
    renderPage("/budget?tab=planning");
    fireEvent.click(await screen.findByRole("button", { name: "Submit" }));
    await waitFor(() =>
      expect(budget.transitionBudgetVersionLifecycle).toHaveBeenCalledWith({
        version_id: 1,
        expected_row_version: 1,
        next_status: "submitted",
      }),
    );
  });

  it("locks the lines of an approved version", async () => {
    perms.granted = new Set([P.FIN_VIEW, P.FIN_BUDGET]);
    budget.listBudgetVersions.mockResolvedValue([
      version({ id: 7, status: "approved", scenario_type: "approved" }),
    ]);
    renderPage("/budget?tab=planning");
    expect(await screen.findByText(/lines can no longer be edited/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New line" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reforecast" })).toBeInTheDocument();
  });
});

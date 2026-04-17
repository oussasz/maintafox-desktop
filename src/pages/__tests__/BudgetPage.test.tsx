import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BudgetPage } from "@/pages/BudgetPage";

const budgetMocks = vi.hoisted(() => ({
  listCostCenters: vi.fn(),
  createCostCenter: vi.fn(),
  updateCostCenter: vi.fn(),
  listBudgetVersions: vi.fn(),
  createBudgetVersion: vi.fn(),
  createBudgetSuccessorVersion: vi.fn(),
  updateBudgetVersion: vi.fn(),
  transitionBudgetVersionLifecycle: vi.fn(),
  listBudgetLines: vi.fn(),
  createBudgetLine: vi.fn(),
  updateBudgetLine: vi.fn(),
  listBudgetActuals: vi.fn(),
  createBudgetActual: vi.fn(),
  postBudgetActual: vi.fn(),
  reverseBudgetActual: vi.fn(),
  listBudgetCommitments: vi.fn(),
  createBudgetCommitment: vi.fn(),
  listForecastRuns: vi.fn(),
  listBudgetForecasts: vi.fn(),
  generateBudgetForecasts: vi.fn(),
  listBudgetVarianceReviews: vi.fn(),
  createBudgetVarianceReview: vi.fn(),
  transitionBudgetVarianceReview: vi.fn(),
  listBudgetDashboardRows: vi.fn(),
  listBudgetDashboardDrilldown: vi.fn(),
  listBudgetAlertConfigs: vi.fn(),
  createBudgetAlertConfig: vi.fn(),
  evaluateBudgetAlerts: vi.fn(),
  listBudgetAlertEvents: vi.fn(),
  acknowledgeBudgetAlert: vi.fn(),
  buildBudgetReportPack: vi.fn(),
  exportBudgetReportPack: vi.fn(),
  importErpCostCenterMaster: vi.fn(),
  exportPostedActualsForErp: vi.fn(),
  exportApprovedReforecastsForErp: vi.fn(),
}));

const permissionState = vi.hoisted(() => ({
  canBudget: true,
  canReport: true,
  canPost: true,
}));

vi.mock("@/services/budget-service", () => budgetMocks);

vi.mock("@/services/org-node-service", () => ({
  listOrgTree: vi.fn().mockResolvedValue([
    {
      node: { id: 10, code: "SITE-A", name: "Site Alpha" },
      can_carry_cost_center: true,
    },
  ]),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (name: string) =>
      name === "fin.report" ? permissionState.canReport : name === "fin.post" ? permissionState.canPost : false,
    canAny: (...names: string[]) =>
      permissionState.canBudget && names.some((name) => name === "fin.budget" || name === "fin.manage"),
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

describe("BudgetPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    permissionState.canBudget = true;
    permissionState.canReport = true;
    permissionState.canPost = true;

    budgetMocks.listCostCenters.mockResolvedValue([
      {
        id: 1,
        code: "CC-OPS",
        name: "Operations",
        entity_id: 10,
        entity_name: "Site Alpha",
        parent_cost_center_id: null,
        parent_cost_center_code: null,
        budget_owner_id: null,
        erp_external_id: "ERP-OPS",
        is_active: 1,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    budgetMocks.listBudgetVersions.mockResolvedValue([
      {
        id: 7,
        fiscal_year: 2026,
        scenario_type: "approved",
        version_no: 1,
        status: "draft",
        currency_code: "EUR",
        title: "FY26 Baseline",
        planning_basis: "Annual baseline",
        source_basis_mix_json: "{\"pm\":0.7}",
        labor_assumptions_json: "{\"headcount\":12}",
        baseline_reference: "FY26-BASE",
        erp_external_ref: "ERP-BGT-26",
        successor_of_version_id: null,
        created_by_id: 1,
        approved_at: null,
        approved_by_id: null,
        frozen_at: null,
        frozen_by_id: null,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    budgetMocks.listBudgetLines.mockResolvedValue([
      {
        id: 100,
        budget_version_id: 7,
        cost_center_id: 1,
        cost_center_code: "CC-OPS",
        cost_center_name: "Operations",
        period_month: 1,
        budget_bucket: "labor",
        planned_amount: 1000,
        source_basis: "manual",
        justification_note: "January",
        asset_family: null,
        work_category: "preventive",
        shutdown_package_ref: null,
        team_id: 10,
        skill_pool_id: null,
        labor_lane: "regular",
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    budgetMocks.listBudgetActuals.mockResolvedValue([]);
    budgetMocks.listBudgetCommitments.mockResolvedValue([]);
    budgetMocks.listForecastRuns.mockResolvedValue([]);
    budgetMocks.listBudgetForecasts.mockResolvedValue([]);
    budgetMocks.listBudgetVarianceReviews.mockResolvedValue([]);
    budgetMocks.listBudgetDashboardRows.mockResolvedValue([]);
    budgetMocks.listBudgetDashboardDrilldown.mockResolvedValue([]);
    budgetMocks.listBudgetAlertConfigs.mockResolvedValue([]);
    budgetMocks.listBudgetAlertEvents.mockResolvedValue([]);
    budgetMocks.exportPostedActualsForErp.mockResolvedValue([]);
    budgetMocks.exportApprovedReforecastsForErp.mockResolvedValue([]);
  });

  it("renders the governed baseline workspace with loaded data", async () => {
    render(<BudgetPage />);

    await waitFor(() => {
      expect(screen.getByText("Budget baseline authoring")).toBeInTheDocument();
    });

    expect(screen.getByText("CC-OPS")).toBeInTheDocument();
    expect(screen.getByText(/2026 \/ approved \/ v1/i)).toBeInTheDocument();
  });

  it("hides editing when budget mutation permission is missing", async () => {
    permissionState.canBudget = false;

    render(<BudgetPage />);

    await waitFor(() => {
      expect(screen.getByText("View only")).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: "Create version" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create line" })).toBeDisabled();
  });

  it("makes frozen baselines read-only even when mutation permission exists", async () => {
    budgetMocks.listBudgetVersions.mockResolvedValue([
      {
        id: 7,
        fiscal_year: 2026,
        scenario_type: "approved",
        version_no: 1,
        status: "frozen",
        currency_code: "EUR",
        title: "Frozen Baseline",
        planning_basis: "Frozen",
        source_basis_mix_json: null,
        labor_assumptions_json: null,
        baseline_reference: "FROZEN-1",
        erp_external_ref: null,
        successor_of_version_id: null,
        created_by_id: 1,
        approved_at: "2026-01-02T00:00:00Z",
        approved_by_id: 1,
        frozen_at: "2026-01-03T00:00:00Z",
        frozen_by_id: 1,
        row_version: 3,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-03T00:00:00Z",
      },
    ]);

    render(<BudgetPage />);

    await waitFor(() => {
      expect(screen.getByText(/2026 \/ approved \/ v1/i)).toBeInTheDocument();
    });

    expect(screen.getByRole("button", { name: "Update draft" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create line" })).toBeDisabled();
  });

  it("submits lifecycle transition requests through the finance service", async () => {
    budgetMocks.transitionBudgetVersionLifecycle.mockResolvedValue({
      id: 7,
      fiscal_year: 2026,
      scenario_type: "approved",
      version_no: 1,
      status: "submitted",
      currency_code: "EUR",
      title: "FY26 Baseline",
      planning_basis: "Annual baseline",
      source_basis_mix_json: "{\"pm\":0.7}",
      labor_assumptions_json: "{\"headcount\":12}",
      baseline_reference: "FY26-BASE",
      erp_external_ref: "ERP-BGT-26",
      successor_of_version_id: null,
      created_by_id: 1,
      approved_at: null,
      approved_by_id: null,
      frozen_at: null,
      frozen_by_id: null,
      row_version: 2,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T01:00:00Z",
    });

    render(<BudgetPage />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Submit" })).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Submit" }));

    await waitFor(() => {
      expect(budgetMocks.transitionBudgetVersionLifecycle).toHaveBeenCalledWith({
        version_id: 7,
        expected_row_version: 1,
        next_status: "submitted",
      });
    });
  });

  it("enforces fin.report gate for ERP alignment actions", async () => {
    permissionState.canReport = false;

    render(<BudgetPage />);
    await waitFor(() => {
      expect(screen.getByText("Budget baseline authoring")).toBeInTheDocument();
    });

    expect(screen.getByText("No export scope detected.")).toBeInTheDocument();
  });
});
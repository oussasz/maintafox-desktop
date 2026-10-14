import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PmPage } from "@/pages/PmPage";
import { usePmStore } from "@/stores/pm-store";
import { mockInvoke } from "@/test/mocks/tauri";

vi.mock(
  "react-i18next",
  async () => (await import("@/components/pm/__tests__/pm-test-i18n")).pmI18nMock,
);
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const TS = "2026-10-01T10:00:00Z";

const plan = {
  id: 1,
  code: "PM-PUMP",
  title: "Graissage pompe",
  description: null,
  asset_scope_type: "equipment",
  asset_scope_id: 10,
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
  row_version: 3,
  created_at: TS,
  updated_at: TS,
  asset_code: "P-101",
  asset_name: "Pompe de reprise",
};

function occurrence(id: number, dueAt: string, planCode: string) {
  return {
    id,
    pm_plan_id: 1,
    plan_version_id: 4,
    due_basis: `fixed:${dueAt}`,
    due_at: dueAt,
    due_meter_value: null,
    generated_at: TS,
    status: "scheduled",
    linked_work_order_id: null,
    linked_work_order_code: null,
    deferral_reason: null,
    missed_reason: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    plan_code: planCode,
    plan_title: "Graissage pompe",
    strategy_type: "fixed",
    asset_code: "P-101",
    asset_name: "Pompe de reprise",
  };
}

const metrics = {
  as_of: TS,
  overdue_count: 1,
  due_today_count: 0,
  due_next_7d_count: 0,
  ready_for_scheduling_count: 0,
};

const generationStatus = {
  control: {
    id: 1,
    enabled: true,
    cron_expr: "0 0 0 * * *",
    horizon_days: 30,
    last_run_at: null,
    next_run_at: null,
    last_status: null,
    last_generated_count: null,
    last_skipped_count: null,
    last_error: null,
    updated_at: TS,
  },
  latest_run: null,
};

const rate = { numerator: 0, denominator: 0, value_pct: null, derivation: "" };

function routeInvoke() {
  mockInvoke.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "list_pm_plans":
        return [plan];
      case "list_pm_occurrences":
        return [
          occurrence(1, "2020-01-01T00:00:00Z", "PM-LATE"),
          occurrence(2, "2099-01-01T00:00:00Z", "PM-FUTURE"),
        ];
      case "get_pm_due_metrics":
        return metrics;
      case "get_pm_generation_status":
        return generationStatus;
      case "list_pm_plan_versions":
      case "list_pm_executions":
      case "list_pm_recurring_findings":
      case "list_reference_domains":
        return [];
      case "list_pm_planning_readiness":
        return {
          as_of: TS,
          candidate_count: 0,
          ready_count: 0,
          blocked_count: 0,
          derivation_rules: [],
          candidates: [],
        };
      case "get_pm_governance_kpi_report":
        return {
          as_of: TS,
          from: TS,
          to: TS,
          pm_plan_id: 1,
          criticality_code: null,
          compliance: rate,
          overdue_risk: rate,
          first_pass_completion: rate,
          follow_up_ratio: rate,
          effort_variance: {
            sample_size: 0,
            estimated_hours: 0,
            actual_hours: 0,
            variance_hours: 0,
            variance_pct: null,
            derivation: "",
          },
          derivation_rules: [],
        };
      case "delete_pm_plan":
        return null;
      default:
        return null;
    }
  });
}

const initial = usePmStore.getState();

beforeEach(() => {
  mockInvoke.mockReset();
  usePmStore.setState(initial, true);
  try {
    localStorage.clear();
  } catch {
    /* ignore */
  }
  routeInvoke();
});

function renderPage(path = "/pm") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <PmPage />
    </MemoryRouter>,
  );
}

describe("PmPage", () => {
  it("shows the due KPIs and a tile opens the due queue filtered on its figure", async () => {
    renderPage();
    const overdueTile = await screen.findByRole("button", { name: /^Overdue: 1/ });
    await waitFor(() => expect(screen.getByText("PM-FUTURE")).toBeInTheDocument());
    fireEvent.click(overdueTile);
    await waitFor(() => expect(screen.queryByText("PM-FUTURE")).not.toBeInTheDocument());
    expect(screen.getByText("PM-LATE")).toBeInTheDocument();
  });

  it("deletes a draft plan through a confirmation dialog, never window.confirm", async () => {
    const confirmSpy = vi.spyOn(window, "confirm");
    renderPage("/pm?planId=1");
    const deleteButton = await screen.findByRole("button", { name: "Delete plan" });
    fireEvent.click(deleteButton);
    const dialog = await screen.findByRole("dialog", { name: "Delete PM plan" });
    expect(confirmSpy).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete plan" }));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("delete_pm_plan", {
        planId: 1,
        expectedRowVersion: 3,
      }),
    );
    confirmSpy.mockRestore();
  });

  it("replaces the content with an error state and retries when the workspace fails to load", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_pm_plans") throw { code: "DB_ERROR", message: "base indisponible" };
      return null;
    });
    renderPage();
    const alert = await screen.findByRole("alert");
    routeInvoke();
    fireEvent.click(within(alert).getByRole("button"));
    await waitFor(() => expect(screen.getByText("PM-FUTURE")).toBeInTheDocument());
  });
});

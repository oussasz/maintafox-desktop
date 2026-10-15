import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { PmPlanDetailDialog } from "@/components/pm/PmPlanDetailDialog";
import type { PmPlanRow } from "@/services/pm-service";
import type {
  PmExecution,
  PmFinding,
  PmGovernanceKpiReport,
  PmPlanVersion,
} from "@shared/ipc-types";

vi.mock("react-i18next", async () => (await import("./pm-test-i18n")).pmI18nMock);
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));
vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi
    .fn()
    .mockResolvedValue([{ id: 1, code: "vibration", label: "Vibrations anormales" }]),
}));

const TS = "2026-10-01T10:00:00Z";

const plan: PmPlanRow = {
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
  requires_permit: 1,
  is_active: 1,
  lifecycle_status: "active",
  current_version_id: 100,
  row_version: 1,
  created_at: TS,
  updated_at: TS,
  asset_code: "P-101",
  asset_name: "Pompe de reprise",
};

const version: PmPlanVersion = {
  id: 100,
  pm_plan_id: 1,
  version_no: 2,
  status: "published",
  effective_from: TS,
  effective_to: null,
  trigger_definition_json: "{}",
  task_package_json: null,
  required_parts_json: null,
  required_skills_json: null,
  required_tools_json: null,
  estimated_duration_hours: null,
  estimated_labor_cost: null,
  estimated_parts_cost: null,
  estimated_service_cost: null,
  change_reason: null,
  row_version: 1,
  created_at: TS,
  updated_at: TS,
};

const execution: PmExecution = {
  id: 7,
  pm_occurrence_id: 3,
  work_order_id: 88,
  work_order_code: "OT-2026-0144",
  execution_result: "completed_with_findings",
  executed_at: TS,
  notes: "Joint remplacé",
  actor_id: 1,
  actual_duration_hours: 1.5,
  actual_labor_hours: null,
  created_at: TS,
};

const findings: PmFinding[] = [
  {
    id: 1,
    pm_execution_id: 7,
    finding_type: "WO_TASK_NOK",
    severity: null,
    description: "Contrôler l'étanchéité — fuite",
    follow_up_di_id: null,
    follow_up_work_order_id: null,
    follow_up_di_code: null,
    follow_up_work_order_code: null,
    created_at: TS,
  },
  {
    id: 2,
    pm_execution_id: 7,
    finding_type: "VIBRATION",
    severity: "high",
    description: "Palier côté accouplement",
    follow_up_di_id: 5,
    follow_up_work_order_id: 9,
    follow_up_di_code: "DI-2026-0012",
    follow_up_work_order_code: "OT-2026-0150",
    created_at: TS,
  },
];

const rate = (pct: number | null) => ({
  numerator: 3,
  denominator: 4,
  value_pct: pct,
  derivation: "",
});
const kpis: PmGovernanceKpiReport = {
  as_of: TS,
  from: "2026-07-01T00:00:00Z",
  to: TS,
  pm_plan_id: 1,
  criticality_code: null,
  compliance: rate(75),
  overdue_risk: rate(10),
  first_pass_completion: rate(50),
  follow_up_ratio: rate(null),
  failures_found_by_pm: 0,
  effort_variance: {
    sample_size: 4,
    estimated_hours: 6,
    actual_hours: 7,
    variance_hours: 1,
    variance_pct: 16.7,
    derivation: "",
  },
  derivation_rules: [],
};

function renderDialog(overrides: Partial<Parameters<typeof PmPlanDetailDialog>[0]> = {}) {
  const props: Parameters<typeof PmPlanDetailDialog>[0] = {
    open: true,
    plan,
    versions: [version],
    occurrences: [],
    executions: [execution],
    findingsByExecutionId: { 7: findings },
    recurringFindings: [],
    planningReadiness: null,
    governanceKpis: kpis,
    onRetryDetail: vi.fn(),
    onClose: vi.fn(),
    onEditPlan: vi.fn(),
    onDeletePlan: vi.fn(),
    onLifecycle: vi.fn(),
    onNewVersion: vi.fn(),
    onEditVersion: vi.fn(),
    onPublishVersion: vi.fn(),
    onDeleteVersion: vi.fn(),
    onTransitionOccurrence: vi.fn(),
    onExecuteOccurrence: vi.fn(),
    onLoadFindings: vi.fn(),
    onReviseFromFinding: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(
    <MemoryRouter>
      <PmPlanDetailDialog {...props} />
    </MemoryRouter>,
  );
  return props;
}

function openTab(name: string) {
  const tab = screen.getByRole("tab", { name });
  fireEvent.mouseDown(tab);
  fireEvent.click(tab);
}

describe("PmPlanDetailDialog", () => {
  it("shows the plan's asset in the header and overview", () => {
    renderDialog();
    expect(screen.getAllByText("P-101 — Pompe de reprise").length).toBeGreaterThan(0);
    expect(screen.getByText("P-101")).toBeInTheDocument();
  });

  it("translates the version status and explains why a published version cannot be deleted", () => {
    renderDialog();
    openTab("Versions");
    expect(screen.getByText("Published")).toBeInTheDocument();
    const del = screen.getByRole("button", { name: "Delete version" });
    expect(del).toBeDisabled();
    expect(del).toHaveAttribute("title", "Delete is available only for draft versions.");
  });

  it("lists findings with translated type and severity and links the follow-ups", async () => {
    const props = renderDialog();
    openTab("Executions");
    expect(screen.getByText("Completed — with findings")).toBeInTheDocument();
    expect(screen.getByText("Joint remplacé")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "View findings" }));
    expect(props.onLoadFindings).toHaveBeenCalledWith(7);
    expect(screen.getByText("Checklist step not OK")).toBeInTheDocument();
    expect(screen.getByText("High")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText("Vibrations anormales")).toBeInTheDocument());
    expect(screen.queryByText("WO_TASK_NOK")).not.toBeInTheDocument();
    expect(screen.getByText("linkedEntity.di:DI-2026-0012")).toBeInTheDocument();
    expect(screen.getByText("linkedEntity.work_order:OT-2026-0150")).toBeInTheDocument();
  });

  it("revises the plan from a finding and shows the versions", async () => {
    const props = renderDialog();
    openTab("Executions");
    fireEvent.click(screen.getByRole("button", { name: "View findings" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Revise the plan" })[1] as HTMLElement);
    expect(props.onReviseFromFinding).toHaveBeenCalledWith(expect.objectContaining({ id: 2 }));
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Versions" })).toHaveAttribute("data-state", "active"),
    );
  });

  it("opens on the occurrence named by a notification", () => {
    const occurrence = {
      id: 31,
      pm_plan_id: 1,
      plan_version_id: 100,
      due_basis: "fixed:x",
      due_at: "2099-03-10T00:00:00Z",
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
      plan_code: "PM-PUMP",
      plan_title: "Graissage pompe",
      strategy_type: "fixed",
      asset_code: "P-101",
      asset_name: "Pompe de reprise",
      window_start_at: "2099-03-09T00:00:00Z",
      window_end_at: "2099-03-12T23:59:59Z",
    };
    renderDialog({ occurrences: [occurrence], focusOccurrenceId: 31 });
    const focus = screen.getByRole("region", { name: "Occurrence from the notification" });
    expect(focus).toHaveTextContent(/Window/);
    expect(screen.getByRole("tab", { name: "Occurrences" })).toHaveAttribute(
      "data-state",
      "active",
    );
  });

  it("labels the governance KPIs in the app language", () => {
    renderDialog();
    openTab("Governance");
    expect(screen.getByText("PM compliance")).toBeInTheDocument();
    expect(screen.getByText("Overdue risk")).toBeInTheDocument();
    expect(screen.getByText("Completed without findings")).toBeInTheDocument();
    expect(screen.getByText("Findings with a follow-up")).toBeInTheDocument();
    expect(screen.getByText("Effort variance")).toBeInTheDocument();
    expect(screen.queryByText(/Compliance:/)).not.toBeInTheDocument();
  });

  it("shows a detail load failure with a retry", () => {
    const props = renderDialog({ detailError: { code: "DB_ERROR", message: "boom" } });
    fireEvent.click(screen.getByRole("button", { name: /retry|ds\./i }));
    expect(props.onRetryDetail).toHaveBeenCalled();
  });
});

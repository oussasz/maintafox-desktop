/**
 * Site scope of the PM plan screen: the plan actions answer with the rights at the plan's site
 * (`access_node_id`), not the "held anywhere" set; a family / criticality-group plan (no node) is
 * read at node 0, the plant-wide rights only.
 */
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PmPlanDetailDialog } from "@/components/pm/PmPlanDetailDialog";
import type { PmPlanRow } from "@/services/pm-service";
import { P } from "@shared/rbac/permissions.generated";

import { pmPlanAccessNode } from "../pm-site-scope";

vi.mock("react-i18next", async () => (await import("./pm-test-i18n")).pmI18nMock);
// The "held anywhere" set: whether an action is offered at all.
const heldAnywhere = vi.hoisted(() => ({ perms: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => heldAnywhere.perms.has(p),
    canAny: (...ps: string[]) => ps.some((p) => heldAnywhere.perms.has(p)),
    isLoading: false,
  }),
}));
vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn().mockResolvedValue([]),
}));

const rightsAt = vi.hoisted(() => ({
  byNode: new Map<number, string[]>(),
  asked: [] as (number | null | undefined)[],
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    rightsAt.asked.push(node);
    const granted = node == null ? [] : (rightsAt.byNode.get(node) ?? []);
    return { loading: false, can: (p: string) => granted.includes(p) };
  },
}));

const TS = "2026-10-01T10:00:00Z";
const OTHER_SITE = "You hold this right only on your own site: this plan is on another site.";
const PLANT_WIDE = "Plan covering several sites: reserved for plant-wide rights.";
const NO_SITE =
  "Plan without a site (its equipment is installed nowhere): reserved for plant-wide rights.";
const SITE_A = 5;
const SITE_B = 6;

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
  requires_permit: 0,
  is_active: 1,
  lifecycle_status: "proposed",
  current_version_id: null,
  row_version: 1,
  created_at: TS,
  updated_at: TS,
  asset_code: "P-101",
  asset_name: "Pompe de reprise",
  access_node_id: SITE_A,
};

function renderDialog(p: PmPlanRow) {
  render(
    <MemoryRouter>
      <PmPlanDetailDialog
        open
        plan={p}
        versions={[]}
        occurrences={[]}
        executions={[]}
        findingsByExecutionId={{}}
        recurringFindings={[]}
        planningReadiness={null}
        governanceKpis={null}
        onRetryDetail={vi.fn()}
        onClose={vi.fn()}
        onEditPlan={vi.fn()}
        onDeletePlan={vi.fn()}
        onDuplicatePlan={vi.fn()}
        onLifecycle={vi.fn()}
        onNewVersion={vi.fn()}
        onEditVersion={vi.fn()}
        onPublishVersion={vi.fn()}
        onDeleteVersion={vi.fn()}
        onTransitionOccurrence={vi.fn()}
        onExecuteOccurrence={vi.fn()}
        onLoadFindings={vi.fn()}
        onReviseFromFinding={vi.fn().mockResolvedValue(undefined)}
      />
    </MemoryRouter>,
  );
}

const ALL_PM = [P.PM_VIEW, P.PM_CREATE, P.PM_EDIT, P.PM_APPROVE, P.PM_DELETE];

beforeEach(() => {
  rightsAt.byNode = new Map();
  rightsAt.asked = [];
  heldAnywhere.perms = new Set(ALL_PM);
});

describe("pmPlanAccessNode", () => {
  it("reads a plan at its node, a plan without one at node 0, nothing while none is loaded", () => {
    expect(pmPlanAccessNode({ access_node_id: SITE_A })).toBe(SITE_A);
    expect(pmPlanAccessNode({ access_node_id: null })).toBe(0);
    expect(pmPlanAccessNode({ access_node_id: undefined })).toBe(0);
    expect(pmPlanAccessNode({ access_node_id: 0 })).toBe(0);
    expect(pmPlanAccessNode(null)).toBeNull();
  });
});

describe("PM plan actions follow the rights at the plan's site", () => {
  it("enables the actions held at the plan's site, disables the others with the reason", () => {
    rightsAt.byNode.set(SITE_A, [P.PM_VIEW, P.PM_EDIT]);
    renderDialog(plan);
    expect(rightsAt.asked).toContain(SITE_A);
    expect(screen.getByRole("button", { name: "Edit plan" })).toBeEnabled();
    // Approval is held elsewhere only: shown, disabled, with why.
    const approve = screen.getByRole("button", { name: "Approve" });
    expect(approve).toBeDisabled();
    expect(approve).toHaveAttribute("title", OTHER_SITE);
    expect(screen.getByTestId("pm-plan-site-reason")).toHaveTextContent(OTHER_SITE);
  });

  it("disables every action on a plan of a site outside the user's roles, with the reason", () => {
    rightsAt.byNode.set(SITE_B, ALL_PM);
    renderDialog(plan);
    for (const name of ["Edit plan", "Approve"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("title", OTHER_SITE);
    }
  });

  it("hides an action whose right is held nowhere", () => {
    heldAnywhere.perms = new Set([P.PM_VIEW, P.PM_EDIT]);
    rightsAt.byNode.set(SITE_A, [P.PM_VIEW, P.PM_EDIT]);
    renderDialog(plan);
    expect(screen.getByRole("button", { name: "Edit plan" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("pm-plan-site-reason")).not.toBeInTheDocument();
  });

  it("reads a family plan with the plant-wide rights (node 0): a site role gets the reason", () => {
    rightsAt.byNode.set(SITE_A, ALL_PM);
    renderDialog({ ...plan, asset_scope_type: "family", access_node_id: null });
    expect(rightsAt.asked).toContain(0);
    const edit = screen.getByRole("button", { name: "Edit plan" });
    expect(edit).toBeDisabled();
    expect(edit).toHaveAttribute("title", PLANT_WIDE);
  });

  it("explains a plan on equipment installed nowhere", () => {
    renderDialog({ ...plan, access_node_id: null });
    expect(screen.getByRole("button", { name: "Edit plan" })).toHaveAttribute("title", NO_SITE);
  });

  it("enables everything for a plant-wide holder on a family plan", () => {
    rightsAt.byNode.set(0, ALL_PM);
    renderDialog({ ...plan, asset_scope_type: "family", access_node_id: null });
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Edit plan" })).toBeEnabled();
    expect(screen.queryByTestId("pm-plan-site-reason")).not.toBeInTheDocument();
  });
});

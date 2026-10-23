/**
 * WO planning without the personnel and inventory view rights: teams and articles are never
 * requested, and the form names the missing rights instead of telling the planner "no teams".
 * "Needs shutdown": confirmed, saved with the version the planner saw, locked outside preparation.
 * Saved planned part lines: cancelled (kept, muted, out of the total) after a confirmation, and
 * switched required / optional while the work order is in planning.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { WoPlanningPanel } from "@/components/wo/WoPlanningPanel";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import type { WorkOrder } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({ t: (key: string) => key, i18n: { language: "en" } }));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const inv = vi.hoisted(() => ({
  checkWoPartStockAvailability: vi.fn().mockResolvedValue([]),
  createProcurementRequisitionFromWoPart: vi.fn(),
  evaluateInventoryUnitCost: vi.fn().mockResolvedValue(null),
  listInventoryArticles: vi.fn().mockResolvedValue([]),
  listInventoryLocations: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/inventory-service", () => inv);
const personnel = vi.hoisted(() => ({
  listActiveTeams: vi.fn().mockResolvedValue([]),
  listTeamMembers: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/personnel-service", () => personnel);
const exec = vi.hoisted(() => ({
  addPart: vi.fn(),
  addTask: vi.fn(),
  addWoTool: vi.fn(),
  cancelPlannedPart: vi.fn(),
  setPlannedPartRequired: vi.fn(),
  listParts: vi.fn().mockResolvedValue([]),
  listTasks: vi.fn().mockResolvedValue([]),
  listWoTools: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/wo-execution-service", () => exec);
const woSvc = vi.hoisted(() => ({
  PERMIT_REQUIREMENT_EDITABLE_STATUSES: new Set<string>(),
  SHUTDOWN_FLAG_EDITABLE_STATUSES: new Set<string>(["draft", "planning", "ready"]),
  emergencyReassignWo: vi.fn(),
  evaluateWoReadiness: vi.fn().mockResolvedValue(null),
  planWo: vi.fn(),
  setWoPermitRequirement: vi.fn(),
  setWoRequiresShutdown: vi.fn(),
}));
vi.mock("@/services/wo-service", () => woSvc);
// Stable values: the panel's effects depend on them.
const stable = vi.hoisted(() => ({
  session: { info: { user_id: 7, display_name: "Planner", username: "planner" } },
  people: { options: [], loading: false, failed: false },
  typeLabels: { typeLabel: (code: string) => code },
}));
vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({ formatCurrency: (v: number) => `${v.toFixed(2)} DA` }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => stable.session }));
// Rights at the work order’s site (`useWoAccess`): granted here.
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: () => true }),
}));
vi.mock("@/components/wo/use-wo-people", () => ({ useWoPeopleOptions: () => stable.people }));
vi.mock("@/components/wo/use-wo-type-labels", () => ({
  useWoTypeLabels: () => stable.typeLabels,
}));
vi.mock("@/components/wo/WoPermitRequirementField", () => ({
  WoPermitRequirementField: () => null,
}));
vi.mock("@/components/wo/WoShiftSelect", () => ({ WoShiftSelect: () => null }));
vi.mock("@/components/common/urgency/UrgencySelect", () => ({ UrgencySelect: () => null }));

function permissionsWithout(...denied: PermissionName[]): PermissionContextValue {
  const can = (p: PermissionName) => !denied.includes(p);
  return {
    permissions: [],
    isLoading: false,
    can,
    canAny: (...ps) => ps.some(can),
    canAll: (...ps) => ps.every(can),
    refresh: vi.fn(),
  };
}

const WO = {
  id: 12,
  code: "OT-0012",
  title: "Replace bearing",
  status_code: "planning",
  row_version: 3,
  type_code: "CORR",
  urgency_code: null,
  planned_start: null,
  planned_end: null,
  assigned_group_id: null,
  primary_responsible_id: null,
} as unknown as WorkOrder;

describe("WoPlanningPanel without the personnel and inventory view rights", () => {
  it("never requests teams or articles, names the missing rights and does not say 'no teams'", async () => {
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout(P.PER_VIEW, P.INV_VIEW)}>
          <WoPlanningPanel wo={WO} canEdit />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );

    expect(await screen.findByTestId("list-access-teams")).toBeInTheDocument();
    expect(screen.getByTestId("list-access-articles")).toBeInTheDocument();
    expect(screen.queryByText("planning.noTeams")).not.toBeInTheDocument();
    await waitFor(() => expect(personnel.listActiveTeams).not.toHaveBeenCalled());
    expect(inv.listInventoryArticles).not.toHaveBeenCalled();
    expect(inv.listInventoryLocations).not.toHaveBeenCalled();
  });
});

describe("WoPlanningPanel — needs shutdown", () => {
  beforeEach(() => {
    woSvc.setWoRequiresShutdown.mockReset();
  });

  function renderPanel(wo: WorkOrder) {
    return render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <WoPlanningPanel wo={wo} canEdit />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
  }

  it("asks for confirmation, then saves the flag with the version the planner saw", async () => {
    woSvc.setWoRequiresShutdown.mockResolvedValueOnce({
      ...WO,
      requires_shutdown: true,
      row_version: 4,
    });
    renderPanel({ ...WO, requires_shutdown: false } as WorkOrder);

    const box = await screen.findByRole("checkbox", { name: "shutdownRequirement.label" });
    expect(box).toBeEnabled();
    fireEvent.click(box);
    // Nothing is saved before the confirmation.
    expect(woSvc.setWoRequiresShutdown).not.toHaveBeenCalled();
    expect(screen.getByText("shutdownRequirement.confirmOnTitle")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "shutdownRequirement.confirmOn" }));

    await waitFor(() =>
      expect(woSvc.setWoRequiresShutdown).toHaveBeenCalledWith({
        woId: 12,
        requiresShutdown: true,
        expectedRowVersion: 3,
      }),
    );
    await waitFor(() => expect(box).toBeChecked());
  });

  it("saves nothing when the planner cancels", async () => {
    renderPanel({ ...WO, requires_shutdown: true } as WorkOrder);
    const box = await screen.findByRole("checkbox", { name: "shutdownRequirement.label" });
    expect(box).toBeChecked();
    fireEvent.click(box);
    expect(screen.getByText("shutdownRequirement.confirmOffTitle")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ds.confirm.cancel" }));
    expect(woSvc.setWoRequiresShutdown).not.toHaveBeenCalled();
    expect(box).toBeChecked();
  });

  it("is locked with the reason once the work order is no longer being prepared", async () => {
    renderPanel({ ...WO, status_code: "in_progress", requires_shutdown: true } as WorkOrder);
    const box = await screen.findByRole("checkbox", { name: "shutdownRequirement.label" });
    expect(box).toBeDisabled();
    expect(screen.getByText("shutdownRequirement.frozen")).toBeInTheDocument();
  });
});

describe("WoPlanningPanel — required / optional part lines", () => {
  it("marks optional lines and asks the question on a new line, unanswered", async () => {
    inv.checkWoPartStockAvailability.mockResolvedValue({
      is_fully_available: true,
      shortages: [],
    });
    exec.listParts.mockResolvedValueOnce([
      {
        id: 31,
        work_order_id: 12,
        article_id: 4,
        article_ref: "JOINT-2",
        quantity_planned: 1,
        quantity_used: null,
        unit_cost: null,
        stock_location_id: 1,
        reservation_id: null,
        quantity_reserved: 0,
        quantity_issued: 0,
        notes: null,
        is_required: false,
      },
    ]);
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <WoPlanningPanel wo={WO} canEdit />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    // In planning the saved line shows its answer and can be switched.
    const saved = await screen.findByRole("radiogroup", { name: "partNecessity.legendFor" });
    expect(saved.querySelectorAll("input:checked")).toHaveLength(1);
    expect(screen.getByRole("radio", { name: "partNecessity.optional" })).toBeChecked();

    fireEvent.click(screen.getByRole("button", { name: "planning.addRow" }));
    const groups = await screen.findAllByRole("radiogroup", { name: "partNecessity.legendFor" });
    expect(groups).toHaveLength(2);
    expect(groups[1]?.querySelectorAll("input:checked")).toHaveLength(0);
    expect(screen.getByText("partNecessity.choose")).toBeInTheDocument();
    inv.checkWoPartStockAvailability.mockResolvedValue([]);
  });
});

function savedPart(overrides: Record<string, unknown> = {}) {
  return {
    id: 41,
    work_order_id: 12,
    article_id: 4,
    article_ref: "BRG-6207",
    quantity_planned: 2,
    quantity_used: null,
    unit_cost: 10,
    stock_location_id: 1,
    reservation_id: 90,
    quantity_reserved: 2,
    quantity_issued: 0,
    notes: null,
    origin: "planned",
    consumption_status: "pending",
    is_required: true,
    unit_symbol: "pc",
    ...overrides,
  };
}

describe("WoPlanningPanel — cancelling and switching saved part lines", () => {
  beforeEach(() => {
    exec.cancelPlannedPart.mockReset();
    exec.setPlannedPartRequired.mockReset();
    inv.checkWoPartStockAvailability.mockResolvedValue({ is_fully_available: true, shortages: [] });
  });

  function renderPanel(wo: WorkOrder = WO) {
    return render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <WoPlanningPanel wo={wo} canEdit />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
  }

  it("cancels a saved line only after the confirmation, keeps it muted and drops it from the total", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    exec.cancelPlannedPart.mockResolvedValueOnce(
      savedPart({ consumption_status: "cancelled", reservation_id: null, quantity_reserved: 0 }),
    );
    renderPanel();

    // The stock unit is shown next to the quantity; the total counts the line.
    expect(await screen.findByText(/planning\.parts\.plannedQty.*\(pc\)/)).toBeInTheDocument();
    expect(screen.getByText("planning.plannedTotalCost: 20.00 DA")).toBeInTheDocument();
    // A saved line is not "removed" locally any more.
    expect(screen.queryByRole("button", { name: "planning.removePart" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "planning.parts.cancelLine" }));
    expect(exec.cancelPlannedPart).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "planning.parts.cancelConfirm" }));

    await waitFor(() => expect(exec.cancelPlannedPart).toHaveBeenCalledWith(41));
    expect(await screen.findByText("planning.parts.cancelledStatus")).toBeInTheDocument();
    expect(screen.getByTestId("wo-part-line-part-41")).toHaveAttribute("data-status", "cancelled");
    expect(screen.getByText("planning.plannedTotalCost: 0.00 DA")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "planning.parts.cancelLine" })).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "partNecessity.legendFor" })).toBeNull();
  });

  it("does nothing when the planner backs out of the confirmation", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "planning.parts.cancelLine" }));
    fireEvent.click(screen.getByRole("button", { name: "ds.confirm.cancel" }));
    expect(exec.cancelPlannedPart).not.toHaveBeenCalled();
    expect(screen.getByTestId("wo-part-line-part-41")).toHaveAttribute("data-status", "pending");
  });

  it("shows a line cancelled earlier muted, without actions", async () => {
    exec.listParts.mockResolvedValueOnce([
      savedPart({ consumption_status: "cancelled", reservation_id: null }),
    ]);
    renderPanel();
    expect(await screen.findByText("planning.parts.cancelledStatus")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "planning.parts.cancelLine" })).toBeNull();
    expect(screen.queryByText("planning.stockReserved")).toBeNull();
    expect(screen.getByText("planning.plannedTotalCost: 0.00 DA")).toBeInTheDocument();
  });

  it("switches a saved line to optional through the backend", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    exec.setPlannedPartRequired.mockResolvedValueOnce(savedPart({ is_required: false }));
    renderPanel();
    fireEvent.click(await screen.findByRole("radio", { name: "partNecessity.optional" }));
    await waitFor(() => expect(exec.setPlannedPartRequired).toHaveBeenCalledWith(41, false));
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: "partNecessity.optional" })).toBeChecked(),
    );
  });

  it("offers neither cancel nor switch once the work order left planning", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    renderPanel({ ...WO, status_code: "ready" } as WorkOrder);
    expect(await screen.findByTestId("wo-part-line-part-41")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "planning.parts.cancelLine" })).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "partNecessity.legendFor" })).toBeNull();
  });

  it("shows a refused cancel inside the confirmation, which stays open, and keeps the line", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    exec.cancelPlannedPart.mockRejectedValueOnce(new Error("Stock already issued for this line"));
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "planning.parts.cancelLine" }));
    fireEvent.click(screen.getByRole("button", { name: "planning.parts.cancelConfirm" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Stock already issued for this line");
    expect(screen.getByRole("dialog")).toContainElement(alert);
    expect(screen.getByTestId("wo-part-line-part-41")).toHaveAttribute("data-status", "pending");
  });

  it("shows a refused required / optional switch on its line and keeps the saved answer", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    exec.setPlannedPartRequired.mockRejectedValueOnce(new Error("The work order left planning"));
    renderPanel();
    fireEvent.click(await screen.findByRole("radio", { name: "partNecessity.optional" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The work order left planning");
    expect(screen.getByTestId("wo-part-line-part-41")).toContainElement(alert);
    expect(screen.getByRole("radio", { name: "partNecessity.required" })).toBeChecked();
  });

  it("offers neither cancel nor switch without the edit right", async () => {
    exec.listParts.mockResolvedValueOnce([savedPart()]);
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <WoPlanningPanel wo={WO} canEdit={false} />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    expect(await screen.findByTestId("wo-part-line-part-41")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "planning.parts.cancelLine" })).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "partNecessity.legendFor" })).toBeNull();
  });

  it("offers neither cancel nor switch on a line with recorded use or added during execution", async () => {
    exec.listParts.mockResolvedValueOnce([
      savedPart(),
      savedPart({ id: 42, quantity_issued: 1 }),
      savedPart({ id: 43, origin: "execution_added" }),
    ]);
    renderPanel();
    expect(await screen.findByTestId("wo-part-line-part-43")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "planning.parts.cancelLine" })).toHaveLength(1);
    expect(screen.getAllByRole("radiogroup", { name: "partNecessity.legendFor" })).toHaveLength(1);
  });
});

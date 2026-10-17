/**
 * WO planning without the personnel and inventory view rights: teams and articles are never
 * requested, and the form names the missing rights instead of telling the planner "no teams".
 */
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

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
vi.mock("@/services/wo-execution-service", () => ({
  addPart: vi.fn(),
  addTask: vi.fn(),
  addWoTool: vi.fn(),
  listParts: vi.fn().mockResolvedValue([]),
  listTasks: vi.fn().mockResolvedValue([]),
  listWoTools: vi.fn().mockResolvedValue([]),
}));
vi.mock("@/services/wo-service", () => ({
  PERMIT_REQUIREMENT_EDITABLE_STATUSES: new Set<string>(),
  emergencyReassignWo: vi.fn(),
  evaluateWoReadiness: vi.fn().mockResolvedValue(null),
  planWo: vi.fn(),
  setWoPermitRequirement: vi.fn(),
}));
// Stable values: the panel's effects depend on them.
const stable = vi.hoisted(() => ({
  session: { info: { user_id: 7, display_name: "Planner", username: "planner" } },
  people: { options: [], loading: false, failed: false },
  typeLabels: { typeLabel: (code: string) => code },
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => stable.session }));
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

/**
 * A planner without the user administration right keeps the planning page: crews and people
 * load, the approvers list is not requested, and the break-in approver says which right is missing
 * (the three lists used to be read together, so one refusal cost the whole page). While the rights
 * load, the saved crew choice is left alone: it is only replaced once the crews were really read.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BreakInDialog } from "@/components/planning/BreakInDialog";
import { WeeklySchedulePanel } from "@/components/planning/WeeklySchedulePanel";
import { readStoredBoardScope, writeStoredBoardScope } from "@/components/planning/planning-model";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { PlanningPage } from "@/pages/PlanningPage";
import { UNAUTHENTICATED_SESSION, useSessionStore } from "@/store/session-store";
import { usePlanningStore } from "@/stores/planning-store";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en", resolvedLanguage: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const personnel = vi.hoisted(() => ({
  listActiveTeams: vi.fn(),
  listPersonnel: vi.fn(),
  listTeamMembers: vi.fn().mockResolvedValue([]),
  getActiveRateCard: vi.fn().mockResolvedValue(null),
}));
vi.mock("@/services/personnel-service", () => personnel);
const rbac = vi.hoisted(() => ({ listUsers: vi.fn() }));
vi.mock("@/services/rbac-service", () => rbac);
vi.mock("@/services/planning-service", () => ({
  cancelScheduleCommitment: vi.fn(),
  releaseScheduleCommitment: vi.fn(),
  approveWeeklySchedule: vi.fn(),
  decideWeeklyUnfreeze: vi.fn(),
  freezeWeeklySchedule: vi.fn(),
  getWeeklySchedule: vi.fn(),
  publishWeeklySchedule: vi.fn(),
  requestWeeklyUnfreeze: vi.fn(),
}));
const org = vi.hoisted(() => ({ listOrgTree: vi.fn() }));
vi.mock("@/services/org-node-service", () => org);

// The board, calendar, rail and schedule have their own tests; only the page's lists matter here.
vi.mock("@/components/planning/PlanningBoard", () => ({ PlanningBoard: () => null }));
vi.mock("@/components/planning/PlanningCalendarView", () => ({ PlanningCalendarView: () => null }));
vi.mock("@/components/planning/PlanningBacklogRail", () => ({ PlanningBacklogRail: () => null }));
vi.mock("@/components/planning/CrewsWithoutSiteBanner", () => ({
  CrewsWithoutSiteBanner: () => null,
}));
vi.mock("@/components/planning/PlanningKpiSection", () => ({ PlanningKpiSection: () => null }));
vi.mock("@/components/planning/ProductionWindowDialog", () => ({
  ProductionWindowDialog: () => null,
}));
vi.mock("@/components/planning/CommitDialog", () => ({ CommitDialog: () => null }));
vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn().mockResolvedValue([]),
}));

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

const withoutUserAdmin = permissionsWithout(P.ADM_USERS);

describe("Planning without the user administration right", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    personnel.listActiveTeams.mockResolvedValue([{ id: 3, code: "MEC", name: "Mechanics" }]);
    personnel.listPersonnel.mockResolvedValue({ items: [], total: 0 });
    rbac.listUsers.mockResolvedValue([]);
    org.listOrgTree.mockResolvedValue([]);
    const noop = vi.fn().mockResolvedValue(undefined);
    usePlanningStore.setState({
      loadBacklog: noop,
      refreshBacklog: noop,
      loadCommitments: noop,
      loadCapacityRules: noop,
      loadPlanningWindows: noop,
      loadCapacityLoad: noop,
      loadGantt: noop,
      loadChangeLog: noop,
      loadBreakIns: noop,
      error: null,
    });
  });

  it("loads the page and the crews, and does not request the approvers", async () => {
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={withoutUserAdmin}>
          <PlanningPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    await waitFor(() => expect(personnel.listActiveTeams).toHaveBeenCalled());
    expect(personnel.listPersonnel).toHaveBeenCalled();
    expect(rbac.listUsers).not.toHaveBeenCalled();
    expect(screen.queryByTestId("list-access-teams")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("the break-in approver names the missing right", () => {
    render(
      <PermissionContext.Provider value={withoutUserAdmin}>
        <BreakInDialog
          open
          onOpenChange={vi.fn()}
          commitments={[]}
          breakIns={[]}
          teamId={null}
          initialCommitmentId={null}
          approvers={[]}
          personnel={[]}
          saving={false}
          onSubmit={vi.fn()}
        />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("list-access-users")).toBeInTheDocument();
  });

  it("the weekly schedule names the organisation right instead of 'no planning site'", () => {
    render(
      <PermissionContext.Provider value={permissionsWithout(P.ORG_VIEW)}>
        <WeeklySchedulePanel />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("list-access-orgNodes")).toBeInTheDocument();
    expect(screen.queryByText("weekly.noPlanningSite")).not.toBeInTheDocument();
    expect(org.listOrgTree).not.toHaveBeenCalled();
  });
});

describe("Planning while the rights load", () => {
  const USER_ID = 41;

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    personnel.listActiveTeams.mockResolvedValue([{ id: 3, code: "MEC", name: "Mechanics" }]);
    personnel.listPersonnel.mockResolvedValue({ items: [], total: 0 });
    rbac.listUsers.mockResolvedValue([]);
    org.listOrgTree.mockResolvedValue([]);
    const noop = vi.fn().mockResolvedValue(undefined);
    usePlanningStore.setState({
      loadBacklog: noop,
      refreshBacklog: noop,
      loadCommitments: noop,
      loadCapacityRules: noop,
      loadPlanningWindows: noop,
      loadCapacityLoad: noop,
      loadGantt: noop,
      loadChangeLog: noop,
      loadBreakIns: noop,
      error: null,
    });
    useSessionStore.setState({
      info: { ...UNAUTHENTICATED_SESSION, is_authenticated: true, user_id: USER_ID },
    });
  });

  it("keeps the saved crew while the rights load, and once the crews are read", async () => {
    writeStoredBoardScope(USER_ID, { kind: "crew", teamId: 3 });
    const loading: PermissionContextValue = { ...permissionsWithout(), isLoading: true };
    const { rerender } = render(
      <MemoryRouter>
        <PermissionContext.Provider value={loading}>
          <PlanningPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    // Nothing is read or decided before the rights are known.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(personnel.listActiveTeams).not.toHaveBeenCalled();
    expect(readStoredBoardScope(USER_ID)).toEqual({ kind: "crew", teamId: 3 });

    rerender(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <PlanningPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    await waitFor(() => expect(personnel.listActiveTeams).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(readStoredBoardScope(USER_ID)).toEqual({ kind: "crew", teamId: 3 }));
  });

  it("does not overwrite the saved crew when the crews cannot be read", async () => {
    writeStoredBoardScope(USER_ID, { kind: "crew", teamId: 3 });
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout(P.PER_VIEW)}>
          <PlanningPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(personnel.listActiveTeams).not.toHaveBeenCalled();
    expect(readStoredBoardScope(USER_ID)).toEqual({ kind: "crew", teamId: 3 });
  });

  it("replaces a saved crew that is no longer in the crews read", async () => {
    writeStoredBoardScope(USER_ID, { kind: "crew", teamId: 99 });
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={permissionsWithout()}>
          <PlanningPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    await waitFor(() => expect(personnel.listActiveTeams).toHaveBeenCalled());
    await waitFor(() => expect(readStoredBoardScope(USER_ID)).toEqual({ kind: "all" }));
  });
});

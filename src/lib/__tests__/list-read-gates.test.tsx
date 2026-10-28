/**
 * AU-57: screens do not ask the server for a list the user may not read; the picker says which
 * right is missing instead (ListAccessNote), and asks normally once the right is held.
 */
import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PersonnelPicker } from "@/components/inspections/InspectionRoundsPanel";
import { WorkOrderReferencePicker } from "@/components/inventory/WorkOrderReferencePicker";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

const wo = vi.hoisted(() => ({ listWos: vi.fn() }));
vi.mock("@/services/wo-service", () => wo);

const per = vi.hoisted(() => ({
  getWorkforceSummaryReport: vi.fn(),
  getWorkforceKpiReport: vi.fn(),
  getWorkforceSkillsGapReport: vi.fn(),
  listPersonnel: vi.fn(),
  listActiveTeams: vi.fn(),
}));
vi.mock("@/services/personnel-service", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...per,
}));

function rights(granted: PermissionName[]): PermissionContextValue {
  return {
    permissions: [],
    isLoading: false,
    can: (p) => granted.includes(p),
    canAny: (...ps) => ps.some((p) => granted.includes(p)),
    canAll: (...ps) => ps.every((p) => granted.includes(p)),
    refresh: async () => {},
  };
}

function withRights(granted: PermissionName[], node: ReactNode) {
  return <PermissionContext.Provider value={rights(granted)}>{node}</PermissionContext.Provider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  wo.listWos.mockResolvedValue({ items: [], total: 0 });
});

describe("list read gates (AU-57)", () => {
  it("work-order picker: no ot.view, no list_wo, the note instead", async () => {
    render(
      withRights(
        [P.INV_VIEW],
        <WorkOrderReferencePicker id="wo" value={null} onChange={vi.fn()} />,
      ),
    );
    expect(screen.getByTestId("list-access-workOrders")).toHaveTextContent("listAccess.workOrders");
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(wo.listWos).not.toHaveBeenCalled();
  });

  it("work-order picker: with ot.view it searches", async () => {
    render(
      withRights([P.OT_VIEW], <WorkOrderReferencePicker id="wo" value={null} onChange={vi.fn()} />),
    );
    await waitFor(() => expect(wo.listWos).toHaveBeenCalled());
    expect(screen.queryByTestId("list-access-workOrders")).not.toBeInTheDocument();
  });

  it("inspection assignee: no per.view, no person search", () => {
    render(withRights([P.INS_VIEW], <PersonnelPicker id="a" value={null} onChange={vi.fn()} />));
    expect(screen.getByTestId("list-access-people")).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("workforce analyses: no per.report, no report requested", async () => {
    render(withRights([P.PER_VIEW], <WorkforceReportPanel />));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(per.getWorkforceSummaryReport).not.toHaveBeenCalled();
    expect(per.getWorkforceKpiReport).not.toHaveBeenCalled();
    expect(per.getWorkforceSkillsGapReport).not.toHaveBeenCalled();
  });
});

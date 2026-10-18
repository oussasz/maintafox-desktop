/**
 * The cost-centre dialog without the organisation view right: the tree is not requested (a
 * refused request used to show as an error in the dialog) and the field names the missing right.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { CostCenterDialog } from "@/components/finance/BudgetPlanningPanel";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({ t: (key: string) => key, i18n: { language: "en" } }));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const org = vi.hoisted(() => ({ listOrgTree: vi.fn().mockResolvedValue([]) }));
vi.mock("@/services/org-node-service", () => org);

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

describe("CostCenterDialog without the organisation view right", () => {
  it("does not request the tree and names the missing right", () => {
    render(
      <PermissionContext.Provider value={permissionsWithout(P.ORG_VIEW)}>
        <CostCenterDialog center={null} centers={[]} onClose={vi.fn()} onSaved={vi.fn()} />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("list-access-orgNodes")).toBeInTheDocument();
    expect(screen.queryByText("planning.orgNodeHint")).not.toBeInTheDocument();
    expect(org.listOrgTree).not.toHaveBeenCalled();
  });
});

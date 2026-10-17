/**
 * Work-order and PM pickers fed by the inventory: without the inventory view right the article
 * list is not requested and the picker names the missing right instead of looking empty.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PmPlanFormDialog } from "@/components/pm/PmPlanFormDialog";
import { PmPartsPicker } from "@/components/pm/PmVersionPickers";
import { WoAddExecutionPartDialog } from "@/components/wo/WoAddExecutionPartDialog";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const inv = vi.hoisted(() => ({
  listInventoryArticles: vi.fn(),
  listInventoryStockBalances: vi.fn(),
}));
vi.mock("@/services/inventory-service", () => inv);
vi.mock("@/services/wo-execution-service", () => ({ addPart: vi.fn() }));
vi.mock("@/services/lookup-service", () => ({ getLookupValues: vi.fn().mockResolvedValue([]) }));
vi.mock("@/services/pm-service", () => ({
  listPmAssetMeters: vi.fn().mockResolvedValue([]),
  listPmScopeOptions: vi.fn().mockResolvedValue([]),
  previewPmScopeAssets: vi.fn().mockResolvedValue([]),
}));
const personnel = vi.hoisted(() => ({ listActiveTeams: vi.fn() }));
vi.mock("@/services/personnel-service", () => personnel);
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/assets/AssetPicker", () => ({ AssetPicker: () => null }));
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

const withoutInventory = permissionsWithout(P.INV_VIEW);

describe("pickers fed by the inventory, without the inventory view right", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    inv.listInventoryArticles.mockResolvedValue([]);
  });

  it("the work-order part dialog opens on non-stock parts and explains the stock choice", () => {
    render(
      <PermissionContext.Provider value={withoutInventory}>
        <WoAddExecutionPartDialog woId={1} open onOpenChange={vi.fn()} onAdded={vi.fn()} />
      </PermissionContext.Provider>,
    );
    expect(inv.listInventoryArticles).not.toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: "execution.parts.add.modeNonStock" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.click(screen.getByRole("radio", { name: "execution.parts.add.modeStock" }));
    expect(screen.getByTestId("list-access-articles")).toBeInTheDocument();
  });

  it("the PM parts picker does not search and names the missing right", () => {
    render(
      <PermissionContext.Provider value={withoutInventory}>
        <PmPartsPicker value={[{ articleCode: "BRG-01", quantity: 2 }]} onChange={vi.fn()} />
      </PermissionContext.Provider>,
    );
    expect(screen.getByLabelText("fields.addPart")).toBeDisabled();
    expect(screen.getByTestId("list-access-articles")).toBeInTheDocument();
    // The part already on the version keeps its code.
    expect(screen.getByText("BRG-01")).toBeInTheDocument();
    expect(inv.listInventoryArticles).not.toHaveBeenCalled();
  });

  it("the PM plan form does not read crews without the personnel view right", () => {
    personnel.listActiveTeams.mockResolvedValue([]);
    render(
      <PermissionContext.Provider value={permissionsWithout(P.PER_VIEW)}>
        <PmPlanFormDialog
          open
          mode="create"
          onClose={vi.fn()}
          onCreate={vi.fn()}
          onUpdate={vi.fn()}
        />
      </PermissionContext.Provider>,
    );
    expect(personnel.listActiveTeams).not.toHaveBeenCalled();
    expect(screen.getByTestId("list-access-teams")).toBeInTheDocument();
  });

  it("with the right, the work-order part dialog loads the articles", () => {
    render(
      <PermissionContext.Provider value={permissionsWithout()}>
        <WoAddExecutionPartDialog woId={1} open onOpenChange={vi.fn()} onAdded={vi.fn()} />
      </PermissionContext.Provider>,
    );
    expect(inv.listInventoryArticles).toHaveBeenCalled();
  });
});

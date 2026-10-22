/**
 * AU-83: a list of another module shown in Données de référence opens only with that module's
 * read right; otherwise the pane names the missing right and the module's commands are not called.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { SyntheticDomainHost } from "@/pages/ReferenceManagerPage";
import {
  FMECA_RATING_SCALES_DOMAIN_ID,
  INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  PERMIT_TYPES_DOMAIN_ID,
  URGENCY_SCALE_DOMAIN_ID,
  syntheticCatalogReadList,
} from "@/stores/reference-manager-store";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
vi.mock("@/components/lookups/adapters/InventoryArticleFamilyHost", () => ({
  InventoryArticleFamilyHost: () => <div data-testid="article-family-host" />,
}));
vi.mock("@/components/lookups/adapters/FmecaRatingScalesHost", () => ({
  FmecaRatingScalesHost: () => <div data-testid="fmeca-host" />,
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

describe("Données de référence: other modules' lists (AU-83)", () => {
  it("names the module right each such list needs", () => {
    expect(syntheticCatalogReadList(INVENTORY_ARTICLE_FAMILY_DOMAIN_ID)).toBe("stockCatalogs");
    expect(syntheticCatalogReadList(INVENTORY_TAX_CATEGORY_DOMAIN_ID)).toBe("stockCatalogs");
    expect(syntheticCatalogReadList(FMECA_RATING_SCALES_DOMAIN_ID)).toBe("fmecaScales");
    expect(syntheticCatalogReadList(PERMIT_TYPES_DOMAIN_ID)).toBe("permitTypes");
    expect(syntheticCatalogReadList(URGENCY_SCALE_DOMAIN_ID)).toBeNull();
  });

  it("shows the missing right instead of the stock list without inv.view", () => {
    render(
      <PermissionContext.Provider value={rights([P.REF_VIEW])}>
        <SyntheticDomainHost domainId={INVENTORY_ARTICLE_FAMILY_DOMAIN_ID} />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("list-access-stockCatalogs")).toBeInTheDocument();
    expect(screen.queryByTestId("article-family-host")).not.toBeInTheDocument();
  });

  it("opens the FMECA scales with ram.view", () => {
    render(
      <PermissionContext.Provider value={rights([P.REF_VIEW, P.RAM_VIEW])}>
        <SyntheticDomainHost domainId={FMECA_RATING_SCALES_DOMAIN_ID} />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("fmeca-host")).toBeInTheDocument();
  });
});

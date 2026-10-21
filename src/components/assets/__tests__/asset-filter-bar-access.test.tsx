/**
 * Without the organisation view right the asset registry does not ask for the organisation tree
 * (refused by the server, which used to break every filter's options) and hides the location
 * filter; the other filters keep their options.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetFilterBar } from "@/components/assets/AssetFilterBar";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const org = vi.hoisted(() => ({ listOrgTree: vi.fn() }));
vi.mock("@/services/org-node-service", () => org);
const ref = vi.hoisted(() => ({ listPublishedReferenceValuesByDomainCode: vi.fn() }));
vi.mock("@/services/reference-service", () => ref);

const captured = vi.hoisted(() => ({ ids: [] as string[] }));
vi.mock("@/components/filters/SmartFilterBar", () => ({
  SmartFilterBar: ({ filters }: { filters: { id: string; options: unknown[] }[] }) => {
    captured.ids = filters.map((f) => `${f.id}:${f.options.length}`);
    return <div data-testid="filters">{captured.ids.join(",")}</div>;
  },
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

describe("AssetFilterBar without the organisation view right", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ref.listPublishedReferenceValuesByDomainCode.mockResolvedValue([
      { code: "PUMP", label: "Pump" },
    ]);
    org.listOrgTree.mockResolvedValue([]);
  });

  it("does not request the organisation tree, hides the location filter and keeps the others", async () => {
    render(
      <PermissionContext.Provider value={permissionsWithout(P.ORG_VIEW)}>
        <AssetFilterBar />
      </PermissionContext.Provider>,
    );
    await waitFor(() => expect(screen.getByTestId("filters")).toHaveTextContent("class:1"));
    expect(org.listOrgTree).not.toHaveBeenCalled();
    expect(captured.ids.some((id) => id.startsWith("location"))).toBe(false);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("offers the location filter with the right", async () => {
    render(
      <PermissionContext.Provider value={permissionsWithout()}>
        <AssetFilterBar />
      </PermissionContext.Provider>,
    );
    await waitFor(() => expect(org.listOrgTree).toHaveBeenCalled());
    expect(captured.ids.some((id) => id.startsWith("location"))).toBe(true);
  });
});

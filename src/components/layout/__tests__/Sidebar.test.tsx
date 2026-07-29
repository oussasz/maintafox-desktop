import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { Sidebar } from "@/components/layout/Sidebar";
import { defaultNavItems, appRoutes } from "@/navigation/nav-registry";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const mockCan = vi.fn((_permission: PermissionName) => true);

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (permission: PermissionName) => mockCan(permission),
    canAny: (...permissions: PermissionName[]) => permissions.some((p) => mockCan(p)),
    canAll: (...permissions: PermissionName[]) => permissions.every((p) => mockCan(p)),
    permissions: [],
    isLoading: false,
    refresh: async () => undefined,
  }),
}));

vi.mock("@/hooks/use-module-capabilities", () => ({
  useModuleCapabilities: () => ({ capabilityMap: {}, loading: false }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

const ALL_PERMISSIONS = [
  ...new Set(
    defaultNavItems
      .filter((i): i is typeof i & { requiredPermission: PermissionName } => !!i.requiredPermission)
      .map((i) => i.requiredPermission),
  ),
];

function renderSidebar() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Sidebar items={defaultNavItems} />
    </MemoryRouter>,
  );
}

describe("Sidebar — permission-based filtering", () => {
  beforeEach(() => {
    mockCan.mockReset();
    mockCan.mockImplementation(() => true);
  });

  it("V1 — Admin sees all 27 nav items", async () => {
    mockCan.mockImplementation((permission: PermissionName) =>
      ALL_PERMISSIONS.includes(permission),
    );
    renderSidebar();

    await waitFor(() => {
      const links = screen.getAllByRole("link");
      expect(links).toHaveLength(appRoutes.length);
    });

    expect(screen.getAllByRole("link")).toHaveLength(appRoutes.length);
  });

  it("V2 — Non-admin sees only permitted modules (unauthorized hidden, not greyed)", async () => {
    const allowed = new Set<PermissionName>([P.EQ_VIEW, P.DI_VIEW]);
    mockCan.mockImplementation((permission: PermissionName) => allowed.has(permission));
    renderSidebar();

    await waitFor(() => {
      expect(screen.getAllByRole("link")).toHaveLength(5);
    });

    expect(screen.getByText("nav.dashboard")).toBeInTheDocument();
    expect(screen.getByText("nav.equipment")).toBeInTheDocument();
    expect(screen.getByText("nav.requests")).toBeInTheDocument();
    expect(screen.getByText("nav.notifications")).toBeInTheDocument();
    expect(screen.getByText("nav.profile")).toBeInTheDocument();

    expect(screen.queryByText("nav.workOrders")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.users")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.settings")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.planning")).not.toBeInTheDocument();
  });

  it("Dashboard, Profile, and Notifications are always visible regardless of role", async () => {
    mockCan.mockImplementation(() => false);
    renderSidebar();

    await waitFor(() => {
      expect(screen.getAllByRole("link")).toHaveLength(3);
    });

    expect(screen.getByText("nav.dashboard")).toBeInTheDocument();
    expect(screen.getByText("nav.notifications")).toBeInTheDocument();
    expect(screen.getByText("nav.profile")).toBeInTheDocument();
  });

  it("empty groups are hidden when all children are filtered out", async () => {
    mockCan.mockImplementation((permission: PermissionName) => permission === P.EQ_VIEW);
    renderSidebar();

    await waitFor(() => {
      expect(screen.getAllByRole("link")).toHaveLength(4);
    });

    expect(screen.queryByText("nav.groups.planning")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.groups.compliance")).not.toBeInTheDocument();
    expect(screen.getByText("nav.groups.core")).toBeInTheDocument();
  });
});

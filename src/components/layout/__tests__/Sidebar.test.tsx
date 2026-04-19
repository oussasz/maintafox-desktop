import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { Sidebar } from "@/components/layout/Sidebar";
import { defaultNavItems, appRoutes } from "@/navigation/nav-registry";
import { useAppStore } from "@/store/app-store";

// ── Mock rbac-service (same pattern as use-permissions.test.ts) ───────────

const mockGetMyPermissions = vi.fn();

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

// ── i18n: passthrough — return the key as display text ────────────────────

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

// ── All unique permission names used by the nav registry ──────────────────

const ALL_PERMISSIONS = [
  ...new Set(
    defaultNavItems
      .filter((i): i is typeof i & { requiredPermission: string } => !!i.requiredPermission)
      .map((i) => i.requiredPermission),
  ),
];

/** Build a PermissionRecord[] array for the given permission names. */
function makePermissions(names: string[]) {
  return names.map((name) => ({
    name,
    description: "",
    category: "",
    is_dangerous: false,
    requires_step_up: false,
  }));
}

/** Render Sidebar inside a MemoryRouter so useLocation / Link work. */
function renderSidebar() {
  useAppStore.setState({ sidebarCollapsed: false, sidebarHoverOpen: false });
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Sidebar items={defaultNavItems} />
    </MemoryRouter>,
  );
}

// ── Tests ─────────────────────────────────────────────────────────────────

describe("Sidebar — permission-based filtering", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
  });

  it("V1 — Admin sees all 27 nav items", async () => {
    // Admin has every permission
    mockGetMyPermissions.mockResolvedValue(makePermissions(ALL_PERMISSIONS));
    renderSidebar();

    // Wait for usePermissions to resolve
    await waitFor(() => {
      const links = screen.getAllByRole("link");
      expect(links).toHaveLength(appRoutes.length);
    });

    // Verify the count matches the 27 non-header items
    expect(appRoutes).toHaveLength(27);
    expect(screen.getAllByRole("link")).toHaveLength(27);
  });

  it("V2 — Non-admin sees only permitted modules (unauthorized hidden, not greyed)", async () => {
    // Operator-like user: only eq.view and di.view
    mockGetMyPermissions.mockResolvedValue(makePermissions(["eq.view", "di.view"]));
    renderSidebar();

    await waitFor(() => {
      // 2 permitted + 3 always-visible (dashboard, notifications, profile) = 5
      expect(screen.getAllByRole("link")).toHaveLength(5);
    });

    // Authorized items present
    expect(screen.getByText("nav.dashboard")).toBeInTheDocument();
    expect(screen.getByText("nav.equipment")).toBeInTheDocument();
    expect(screen.getByText("nav.requests")).toBeInTheDocument();
    expect(screen.getByText("nav.notifications")).toBeInTheDocument();
    expect(screen.getByText("nav.profile")).toBeInTheDocument();

    // Unauthorized items absent (hidden, not greyed)
    expect(screen.queryByText("nav.workOrders")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.users")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.settings")).not.toBeInTheDocument();
    expect(screen.queryByText("nav.planning")).not.toBeInTheDocument();
  });

  it("Dashboard, Profile, and Notifications are always visible regardless of role", async () => {
    // User with zero permissions
    mockGetMyPermissions.mockResolvedValue([]);
    renderSidebar();

    await waitFor(() => {
      expect(screen.getAllByRole("link")).toHaveLength(3);
    });

    expect(screen.getByText("nav.dashboard")).toBeInTheDocument();
    expect(screen.getByText("nav.notifications")).toBeInTheDocument();
    expect(screen.getByText("nav.profile")).toBeInTheDocument();
  });

  it("empty groups are hidden when all children are filtered out", async () => {
    // Give only eq.view — so Compliance, Planning, Inventory, Analytics, Admin groups are empty
    mockGetMyPermissions.mockResolvedValue(makePermissions(["eq.view"]));
    renderSidebar();

    await waitFor(() => {
      // dashboard + equipment + notifications + profile = 4
      expect(screen.getAllByRole("link")).toHaveLength(4);
    });

    // The "Planning" group header should not appear since no children are visible
    expect(screen.queryByText("nav.groups.planning")).not.toBeInTheDocument();
    // The "Compliance" group header should not appear
    expect(screen.queryByText("nav.groups.compliance")).not.toBeInTheDocument();
    // Core Operations header should still appear (has visible children)
    expect(screen.getByText("nav.groups.core")).toBeInTheDocument();
  });
});

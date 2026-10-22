/**
 * F1 (owner test of 0.9.1): the dashboard showed "La disposition enregistrée n'a pas pu être lue …
 * Session expirée ou absente" and stayed so after signing in again. A lost session now offers
 * "Se reconnecter", and a new session (raised session generation) reloads the saved layout.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { DashboardPage } from "@/pages/DashboardPage";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { useSessionStore } from "@/store/session-store";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr", resolvedLanguage: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const dashboard = vi.hoisted(() => ({
  getDashboardLayout: vi.fn(),
  getOverviewKpiLayout: vi.fn(),
  getOverviewKpis: vi.fn().mockResolvedValue(null),
}));
vi.mock("@/services/dashboard-service", () => dashboard);

// The panels have their own tests; only the saved layout matters here.
vi.mock("@/components/dashboard/DashboardCustomizeDialog", () => ({
  DashboardCustomizeDialog: () => null,
}));
vi.mock("@/components/dashboard/DashboardDiStatusChart", () => ({
  DashboardDiStatusChart: () => null,
}));
vi.mock("@/components/dashboard/DashboardWorkloadChart", () => ({
  DashboardWorkloadChart: () => null,
}));
vi.mock("@/components/dashboard/home/HomeMyWorkPanel", () => ({ HomeMyWorkPanel: () => null }));
vi.mock("@/components/dashboard/home/HomePlanningPanel", () => ({ HomePlanningPanel: () => null }));
vi.mock("@/components/dashboard/home/HomeSupervisionPanel", () => ({
  HomeSupervisionPanel: () => null,
}));

const AUTH_ERROR = {
  code: "AUTH_ERROR",
  message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
};

const permissions: PermissionContextValue = {
  permissions: [],
  isLoading: false,
  can: () => false,
  canAny: () => false,
  canAll: () => false,
  refresh: async () => undefined,
};

function renderPage() {
  return render(
    <MemoryRouter>
      <PermissionContext.Provider value={permissions}>
        <DashboardPage />
      </PermissionContext.Provider>
    </MemoryRouter>,
  );
}

describe("dashboard after a lost session", () => {
  beforeEach(() => {
    useSessionStore.getState().resetForTests();
    useAuthInterceptorStore.getState().clear();
    dashboard.getDashboardLayout.mockReset();
    dashboard.getOverviewKpiLayout.mockReset();
    dashboard.getOverviewKpiLayout.mockResolvedValue({ layout_json: "{}" });
  });

  it("offers to sign in again, and reloads the saved layout once a new session starts", async () => {
    dashboard.getDashboardLayout.mockRejectedValueOnce(AUTH_ERROR);
    renderPage();

    const signIn = await screen.findByTestId("dashboard-sign-in-again");
    expect(signIn).toHaveTextContent("auth:session.expired.action");
    act(() => signIn.click());
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(true);

    // The user signs in again: the session generation goes up and the layout loads.
    dashboard.getDashboardLayout.mockResolvedValue({ layout_json: "{}" });
    act(() => useSessionStore.setState((s) => ({ sessionGeneration: s.sessionGeneration + 1 })));
    await waitFor(() => expect(screen.queryByTestId("dashboard-sign-in-again")).toBeNull());
    expect(dashboard.getDashboardLayout).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("home.layoutError")).toBeNull();
  });

  it("does not offer to sign in again for an error that is not a lost session", async () => {
    dashboard.getDashboardLayout.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "x" });
    renderPage();
    await screen.findByText("home.layoutError");
    expect(screen.queryByTestId("dashboard-sign-in-again")).toBeNull();
  });
});

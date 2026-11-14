/**
 * A start that stays "loading" for too long shows a clear message with a Retry button instead of
 * a spinner that never ends.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AppShell } from "../AppShell";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

vi.mock("../TopBar", () => ({ TopBar: () => <div>top bar</div> }));
vi.mock("../Sidebar", () => ({ Sidebar: () => <nav>side bar</nav> }));
vi.mock("../StatusBar", () => ({ StatusBar: () => <div>status bar</div> }));
vi.mock("@/components/shell/AppToastHost", () => ({ AppToastHost: () => null }));
vi.mock("@/hooks/use-updater", () => ({ useUpdateScheduler: () => undefined }));
vi.mock("@/lib/ipc-invoke", () => ({ invokeSilent: vi.fn(async () => undefined) }));
vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => false }));

const appState = vi.hoisted(() => ({ startupStalled: false }));
vi.mock("@/store/app-store", () => ({
  useAppStore: (sel: (s: unknown) => unknown) =>
    sel({
      appStatus: "loading",
      startupMessage: "",
      startupStalled: appState.startupStalled,
      sidebarCollapsed: false,
    }),
}));
vi.mock("@/store/session-store", () => ({
  useSessionStore: (sel: (s: unknown) => unknown) =>
    sel({ info: { is_authenticated: false, is_locked: false, user_id: null } }),
}));
vi.mock("@/stores/locale-store", () => ({
  useLocaleStore: (sel: (s: unknown) => unknown) => sel({ refreshCurrency: vi.fn() }),
}));
vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) =>
    sel({ initialize: vi.fn(), shutdown: vi.fn() }),
}));

describe("AppShell start-up screen", () => {
  it("shows the spinner message while the start is recent", () => {
    appState.startupStalled = false;
    render(<AppShell>content</AppShell>);
    expect(screen.getByText("startup.loading")).toBeTruthy();
    expect(screen.queryByText("startup.retry")).toBeNull();
  });

  it("offers a Retry button once the start is stalled", () => {
    appState.startupStalled = true;
    const reload = vi.fn();
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...window.location, reload },
    });
    render(<AppShell>content</AppShell>);
    expect(screen.getByText("startup.stalledTitle")).toBeTruthy();
    fireEvent.click(screen.getByText("startup.retry"));
    expect(reload).toHaveBeenCalled();
  });
});

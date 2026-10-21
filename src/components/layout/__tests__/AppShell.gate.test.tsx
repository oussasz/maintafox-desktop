/**
 * While the forced lock removal acknowledgement screen shows, the rest of the app is inert and
 * hidden from screen readers, and no key pressed on the screen reaches the document-level
 * shortcuts (Ctrl+K command palette).
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { isBlockingGateActive } from "@/lib/blocking-gate";

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
vi.mock("@/components/auth/AdminRecoveryNotice", () => ({ AdminRecoveryNotice: () => null }));
vi.mock("@/components/auth/BuiltinAdminWarning", () => ({ BuiltinAdminWarning: () => null }));
vi.mock("@/components/auth/SingleAdministratorWarning", () => ({
  SingleAdministratorWarning: () => null,
}));
vi.mock("@/components/notifications/SafetyRuleNotice", () => ({ SafetyRuleNotice: () => null }));
vi.mock("@/components/shell/AppToastHost", () => ({ AppToastHost: () => null }));
vi.mock("@/hooks/use-startup-bridge", () => ({ useStartupBridge: () => undefined }));
vi.mock("@/components/updates/UpdateBanner", () => ({ UpdateBanner: () => null }));
vi.mock("@/hooks/use-updater", () => ({ useUpdateScheduler: () => undefined }));
vi.mock("@/lib/ipc-invoke", () => ({ invokeSilent: vi.fn(async () => undefined) }));
vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => false }));
vi.mock("@/store/app-store", () => ({
  useAppStore: (sel: (s: unknown) => unknown) =>
    sel({ appStatus: "ready", startupMessage: "", sidebarCollapsed: false }),
}));
vi.mock("@/store/session-store", () => ({
  useSessionStore: (sel: (s: unknown) => unknown) =>
    sel({ info: { is_authenticated: true, is_locked: false, user_id: 7 } }),
}));
const stores = vi.hoisted(() => ({
  refreshCurrency: vi.fn(async () => undefined),
  initialize: vi.fn(),
  shutdown: vi.fn(),
}));
vi.mock("@/stores/locale-store", () => ({
  useLocaleStore: (sel: (s: unknown) => unknown) =>
    sel({ refreshCurrency: stores.refreshCurrency }),
}));
vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) =>
    sel({ initialize: stores.initialize, shutdown: stores.shutdown }),
}));

const svc = vi.hoisted(() => ({
  listMyForcedLockRemovals: vi.fn(),
  acknowledgeForcedLockRemoval: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const item = {
  isolation: {
    id: 1,
    isolation_point: "MCC-1 breaker",
    removed_at: "2026-10-10T17:00:00Z",
    removed_by_name: "Rémi Remover",
    forced_authorized_by_name: "Chef Site",
    forced_reason: "Fin de poste, travailleur parti",
  },
  permit_id: 41,
  permit_code: "PTW-2026-000041",
  permit_status: "closed",
  equipment_label: "PMP-101",
};

describe("AppShell with the forced removal acknowledgement screen", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("makes the shell inert and hidden while the screen shows, and restores it after", async () => {
    svc.listMyForcedLockRemovals.mockResolvedValue([item]);
    svc.acknowledgeForcedLockRemoval.mockResolvedValue(undefined);
    render(
      <AppShell>
        <p>page</p>
      </AppShell>,
    );
    const gate = await screen.findByTestId("forced-removal-gate");
    const shell = screen.getByTestId("app-shell");
    await waitFor(() => expect(shell).toHaveAttribute("inert"));
    expect(shell).toHaveAttribute("aria-hidden", "true");
    expect(shell.contains(gate)).toBe(false);
    expect(isBlockingGateActive()).toBe(true);

    fireEvent.click(within(gate).getByRole("button", { name: "permits:gateAck.acknowledge" }));
    await waitFor(() => expect(screen.queryByTestId("forced-removal-gate")).toBeNull());
    await waitFor(() => expect(shell).not.toHaveAttribute("inert"));
    expect(shell).not.toHaveAttribute("aria-hidden");
    expect(isBlockingGateActive()).toBe(false);
  });

  it("lets no key pressed on the screen reach the document shortcuts (Ctrl+K opens nothing)", async () => {
    svc.listMyForcedLockRemovals.mockResolvedValue([item]);
    const documentKeys = vi.fn();
    document.addEventListener("keydown", documentKeys);
    try {
      render(
        <AppShell>
          <p>page</p>
        </AppShell>,
      );
      const gate = await screen.findByTestId("forced-removal-gate");
      const ack = within(gate).getByRole("button", { name: "permits:gateAck.acknowledge" });
      fireEvent.keyDown(ack, { key: "k", ctrlKey: true });
      fireEvent.keyDown(ack, { key: "k", metaKey: true });
      expect(documentKeys).not.toHaveBeenCalled();
      // And the global shortcut itself stands down while the screen shows.
      expect(isBlockingGateActive()).toBe(true);
      expect(screen.queryByRole("dialog")).toBeNull();
    } finally {
      document.removeEventListener("keydown", documentKeys);
    }
  });
});

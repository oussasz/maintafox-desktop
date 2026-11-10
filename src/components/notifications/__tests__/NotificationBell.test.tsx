import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationBell } from "@/components/notifications/NotificationBell";
import { useSessionStore } from "@/store/session-store";
import { useNotificationStore } from "@/stores/notification-store";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (!opts) return key;
      const vals = Object.entries(opts)
        .filter(([k]) => k !== "defaultValue")
        .map(([k, v]) => `${k}=${String(v)}`);
      return vals.length ? `${key}[${vals.join(",")}]` : key;
    },
    i18n: { language: "en" },
  }),
}));

const granted = vi.hoisted(() => ({ set: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => granted.set.has(p) }),
}));
vi.mock("@/hooks/use-os-toasts", () => ({ useOsToasts: () => undefined }));
// The inbox below the alerts has its own tests.
vi.mock("@/components/notifications/NotificationInbox", () => ({
  NotificationInbox: () => <div data-testid="inbox" />,
}));
vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => true }));

const svc = vi.hoisted(() => ({
  getLateWorkOrderCount: vi.fn(),
  getOverdueRequestCount: vi.fn(),
}));
vi.mock("@/services/bell-alerts-service", () => svc);
vi.mock("@/services/notification-service", () => ({
  getNotificationCounts: vi.fn().mockResolvedValue({
    unread: 10,
    to_acknowledge: 0,
    snoozed: 0,
    by_module: [{ module_code: "sync", unread: 10 }],
  }),
}));

function Where() {
  const loc = useLocation();
  return <p data-testid="where">{`${loc.pathname}${loc.search}`}</p>;
}

/** Radix opens a menu from the keyboard (a click does not reach it under jsdom). */
function openMenu(trigger: HTMLElement) {
  fireEvent.keyDown(trigger, { key: "Enter" });
}

function renderBell() {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<NotificationBell />} />
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  granted.set = new Set();
  svc.getLateWorkOrderCount.mockResolvedValue(3);
  svc.getOverdueRequestCount.mockResolvedValue(2);
  useSessionStore.setState({
    info: { is_authenticated: true, is_locked: false },
  } as never);
  useNotificationStore.setState({
    counts: {
      unread: 10,
      to_acknowledge: 0,
      snoozed: 0,
      by_module: [{ module_code: "sync", unread: 10 }],
    },
    arrivals: 0,
  });
});

describe("NotificationBell business alerts", () => {
  it("shows late work orders and overdue requests, and counts ten conflicts as one", async () => {
    granted.set = new Set([P.OT_VIEW, P.DI_VIEW]);
    renderBell();
    // 10 conflicts -> 1 row, + 1 late-OT row + 1 overdue-request row.
    await waitFor(() => expect(screen.getByLabelText("bell.aria[count=3]")).toBeTruthy());
    openMenu(screen.getByLabelText("bell.aria[count=3]"));
    expect(await screen.findByText("bell.alerts.lateWorkOrders.title[count=3]")).toBeTruthy();
    expect(screen.getByText("bell.alerts.overdueRequests.title[count=2]")).toBeTruthy();
  });

  it("opens the late work orders list from its row", async () => {
    granted.set = new Set([P.OT_VIEW, P.DI_VIEW]);
    renderBell();
    openMenu(await screen.findByLabelText(/^bell\.aria/));
    fireEvent.click(await screen.findByTestId("bell-alert-wo"));
    expect((await screen.findByTestId("where")).textContent).toBe("/work-orders?scope=overdue");
  });

  it("opens the requests list from its row", async () => {
    granted.set = new Set([P.OT_VIEW, P.DI_VIEW]);
    renderBell();
    openMenu(await screen.findByLabelText(/^bell\.aria/));
    fireEvent.click(await screen.findByTestId("bell-alert-di"));
    expect((await screen.findByTestId("where")).textContent).toBe("/requests");
  });

  it("asks for, and shows, nothing the user has no right to", async () => {
    granted.set = new Set([P.DI_VIEW]);
    renderBell();
    openMenu(await screen.findByLabelText(/^bell\.aria/));
    expect(await screen.findByTestId("bell-alert-di")).toBeTruthy();
    expect(screen.queryByTestId("bell-alert-wo")).toBeNull();
    expect(svc.getLateWorkOrderCount).not.toHaveBeenCalled();
  });

  it("shows no alert block without any right or when the figures are zero", async () => {
    renderBell();
    openMenu(await screen.findByLabelText(/^bell\.aria/));
    expect(await screen.findByTestId("inbox")).toBeTruthy();
    expect(screen.queryByTestId("bell-alert-wo")).toBeNull();
    expect(screen.queryByTestId("bell-alert-di")).toBeNull();
    expect(svc.getLateWorkOrderCount).not.toHaveBeenCalled();
    expect(svc.getOverdueRequestCount).not.toHaveBeenCalled();
  });

  it("hides a figure that is zero", async () => {
    granted.set = new Set([P.OT_VIEW, P.DI_VIEW]);
    svc.getLateWorkOrderCount.mockResolvedValue(0);
    renderBell();
    openMenu(await screen.findByLabelText(/^bell\.aria/));
    expect(await screen.findByTestId("bell-alert-di")).toBeTruthy();
    expect(screen.queryByTestId("bell-alert-wo")).toBeNull();
  });
});

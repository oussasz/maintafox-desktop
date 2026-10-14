import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import type { NotificationSummary } from "@/services/notification-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "hours" in opts ? `${key}:${String(opts["hours"])}` : key,
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  listNotifications: vi.fn(),
  markNotificationRead: vi.fn(),
  acknowledgeNotification: vi.fn(),
  snoozeNotification: vi.fn(),
}));
vi.mock("@/services/notification-service", () => svc);

function row(partial: Partial<NotificationSummary>): NotificationSummary {
  return {
    id: 1,
    title: "DI-0042 SLA dépassé",
    body: "Pompe P-101",
    category_code: "di_sla_breach",
    severity: "warning",
    delivery_state: "delivered",
    created_at: new Date(Date.now() - 5 * 60_000).toISOString(),
    read_at: null,
    acknowledged_at: null,
    action_url: "/requests?di=42",
    escalation_level: 0,
    requires_ack: false,
    ...partial,
  };
}

function Where() {
  const loc = useLocation();
  return <p data-testid="where">{`${loc.pathname}${loc.search}`}</p>;
}

function renderInbox(onNavigate = vi.fn(), onChanged = vi.fn()) {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route
          path="/"
          element={<NotificationInbox onNavigate={onNavigate} onChanged={onChanged} />}
        />
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
  return { onNavigate, onChanged };
}

beforeEach(() => {
  vi.clearAllMocks();
  svc.listNotifications.mockResolvedValue([row({})]);
  svc.markNotificationRead.mockResolvedValue(undefined);
  svc.snoozeNotification.mockResolvedValue(undefined);
});

describe("NotificationInbox", () => {
  it("shows translated tabs, category and delivery state, never the raw codes", async () => {
    renderInbox();
    expect(await screen.findByText("DI-0042 SLA dépassé")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "inbox.tabs.unread" })).toBeInTheDocument();
    expect(screen.getByText("category.di_sla_breach")).toBeInTheDocument();
    expect(screen.getByText("deliveryState.delivered")).toBeInTheDocument();
    expect(screen.queryByText("di_sla_breach")).not.toBeInTheDocument();
    expect(screen.queryByText("delivered")).not.toBeInTheDocument();
    // relative time comes from the app formatter, not "5m ago"
    expect(screen.queryByText(/m ago$/)).not.toBeInTheDocument();
  });

  it("opens the source record from the whole row with the normalised link and closes the menu", async () => {
    const { onNavigate, onChanged } = renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: "inbox.openAria" }));
    await waitFor(() =>
      expect(screen.getByTestId("where")).toHaveTextContent("/requests?openDi=42"),
    );
    expect(svc.markNotificationRead).toHaveBeenCalledWith(1);
    expect(onChanged).toHaveBeenCalled();
    expect(onNavigate).toHaveBeenCalledTimes(1);
  });

  it("keeps the inbox usable when an action fails (the global toast reports it)", async () => {
    svc.snoozeNotification.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    const { onChanged } = renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: "inbox.actions.snooze:1" }));
    await waitFor(() => expect(svc.snoozeNotification).toHaveBeenCalledWith(1, 60));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "inbox.actions.snooze:1" })).toBeEnabled(),
    );
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("shows the empty label of the selected tab", async () => {
    svc.listNotifications.mockResolvedValue([]);
    renderInbox();
    expect(await screen.findByText("inbox.empty.all")).toBeInTheDocument();
  });
});

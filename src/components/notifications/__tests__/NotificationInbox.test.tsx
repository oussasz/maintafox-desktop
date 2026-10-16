import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { OPEN_SYNC_CENTER_EVENT } from "@/components/notifications/notification-links";
import type { NotificationSummary } from "@/services/notification-service";
import { useNotificationStore } from "@/stores/notification-store";

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

vi.mock("@/components/common/urgency/use-urgency-scale", () => ({
  useUrgencyScale: () => ({ labelOf: (code: string) => `urgency:${code}` }),
  urgencyTone: () => "danger",
}));

const svc = vi.hoisted(() => ({
  INBOX_VIEWS: ["inbox", "unread", "to_acknowledge", "snoozed", "archived"],
  listNotifications: vi.fn(),
  markNotificationRead: vi.fn(),
  markNotificationUnread: vi.fn(),
  markAllNotificationsRead: vi.fn(),
  acknowledgeNotification: vi.fn(),
  snoozeNotification: vi.fn(),
  archiveNotification: vi.fn(),
  unarchiveNotification: vi.fn(),
  getNotificationCounts: vi.fn(),
}));
vi.mock("@/services/notification-service", () => svc);
vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => true }));

function row(partial: Partial<NotificationSummary>): NotificationSummary {
  return {
    id: 1,
    title: "stored title",
    body: "stored body",
    category_code: "wo_overdue",
    module_code: "ot",
    severity: "warning",
    urgency_code: "emergency",
    message_key: "wo_overdue",
    message_params: { code: "OT-42", title: "Pump P-101", planned_end: null },
    delivery_state: "delivered",
    created_at: new Date(Date.now() - 5 * 60_000).toISOString(),
    read_at: null,
    acknowledged_at: null,
    snoozed_until: null,
    archived_at: null,
    resolved_at: null,
    action_url: "/work-orders?openWo=42",
    escalation_level: 0,
    requires_ack: false,
    event_acknowledged_by: null,
    reason_code: "planner",
    reason_ref: null,
    source_module: "work_orders",
    source_record_id: "42",
    ...partial,
  };
}

function Where() {
  const loc = useLocation();
  return <p data-testid="where">{`${loc.pathname}${loc.search}`}</p>;
}

function renderInbox(variant: "page" | "compact" = "page", onNavigate = vi.fn()) {
  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route path="/" element={<NotificationInbox variant={variant} onNavigate={onNavigate} />} />
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
  return { onNavigate };
}

beforeEach(() => {
  vi.clearAllMocks();
  svc.listNotifications.mockResolvedValue([row({})]);
  svc.markNotificationRead.mockResolvedValue(undefined);
  svc.snoozeNotification.mockResolvedValue(undefined);
  svc.acknowledgeNotification.mockResolvedValue(undefined);
  svc.archiveNotification.mockResolvedValue(undefined);
  svc.markAllNotificationsRead.mockResolvedValue(3);
  svc.getNotificationCounts.mockResolvedValue({
    unread: 1,
    to_acknowledge: 0,
    snoozed: 0,
    by_module: [{ module_code: "ot", unread: 1 }],
  });
  useNotificationStore.setState({
    counts: { unread: 1, to_acknowledge: 0, snoozed: 0, by_module: [] },
    arrivals: 0,
  });
});

describe("NotificationInbox", () => {
  it("renders the message in the reader's language, why it was received and the work urgency", async () => {
    renderInbox();
    expect(
      await screen.findByText(
        "messages.wo_overdue.title[code=OT-42,title=Pump P-101,planned_end=—]",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText("stored title")).not.toBeInTheDocument();
    expect(screen.getByText("reason.planner")).toBeInTheDocument();
    expect(screen.getByText("urgency:emergency")).toBeInTheDocument();
    expect(screen.getByText("module.ot")).toBeInTheDocument();
    // Raw codes never reach the screen.
    expect(screen.queryByText("wo_overdue")).not.toBeInTheDocument();
  });

  it("falls back to the stored text for notifications without a message key", async () => {
    svc.listNotifications.mockResolvedValue([row({ message_key: null, urgency_code: null })]);
    renderInbox();
    expect(await screen.findByText("stored title")).toBeInTheDocument();
    expect(screen.getByText("severity.warning")).toBeInTheDocument();
  });

  it("opens the exact record and marks it read", async () => {
    const { onNavigate } = renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /^inbox\.openAria/ }));
    await waitFor(() =>
      expect(screen.getByTestId("where")).toHaveTextContent("/work-orders?openWo=42"),
    );
    expect(svc.markNotificationRead).toHaveBeenCalledWith(1);
    expect(onNavigate).toHaveBeenCalledTimes(1);
  });

  it("opens the sync center for a synchronisation conflict", async () => {
    svc.listNotifications.mockResolvedValue([
      row({ action_url: "/sync-center", message_key: "sync_conflict", module_code: "sync" }),
    ]);
    const opened = vi.fn();
    window.addEventListener(OPEN_SYNC_CENTER_EVENT, opened);
    renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /^inbox\.openAria/ }));
    await waitFor(() => expect(opened).toHaveBeenCalledTimes(1));
    window.removeEventListener(OPEN_SYNC_CENTER_EVENT, opened);
  });

  it("acknowledges a notification that must be acknowledged and refreshes the counts", async () => {
    svc.listNotifications.mockResolvedValue([row({ requires_ack: true })]);
    renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /inbox\.actions\.acknowledge/ }));
    await waitFor(() => expect(svc.acknowledgeNotification).toHaveBeenCalledWith(1));
    await waitFor(() => expect(svc.getNotificationCounts).toHaveBeenCalled());
  });

  it("shows who acknowledged the event and offers no acknowledge action then", async () => {
    svc.listNotifications.mockResolvedValue([
      row({ requires_ack: true, event_acknowledged_by: "Ali" }),
    ]);
    renderInbox();
    expect(await screen.findByText("inbox.acknowledgedBy[name=Ali]")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /inbox\.actions\.acknowledge/ })).toBeNull();
  });

  it("marks everything read with the current filters", async () => {
    renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /inbox\.actions\.markAllRead/ }));
    await waitFor(() =>
      expect(svc.markAllNotificationsRead).toHaveBeenCalledWith({ view: "inbox" }),
    );
    expect(await screen.findByText("inbox.markedAll[count=3]")).toBeInTheDocument();
  });

  it("archives a notification", async () => {
    renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /inbox\.actions\.archive/ }));
    await waitFor(() => expect(svc.archiveNotification).toHaveBeenCalledWith(1));
  });

  it("groups notifications about the same record", async () => {
    svc.listNotifications.mockResolvedValue([
      row({ id: 1 }),
      row({ id: 2, category_code: "wo_assigned", message_key: "wo_assigned" }),
      row({ id: 3, source_record_id: "7", message_params: { code: "OT-7" } }),
    ]);
    renderInbox();
    await screen.findAllByRole("button", { name: /^inbox\.openAria/ });
    fireEvent.click(screen.getByRole("switch", { name: "inbox.groupByObject" }));
    const header = await screen.findByText(/inbox\.groupCount\[count=2\]/);
    expect(within(header.parentElement as HTMLElement).getByText(/OT-42/)).toBeInTheDocument();
  });

  it("the compact variant lists the unread view without filters", async () => {
    renderInbox("compact");
    await screen.findByRole("button", { name: /^inbox\.openAria/ });
    expect(svc.listNotifications).toHaveBeenCalledWith(
      expect.objectContaining({ view: "unread", limit: 8, offset: 0 }),
    );
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("keeps the inbox usable when an action fails (the global toast reports it)", async () => {
    svc.archiveNotification.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    renderInbox();
    fireEvent.click(await screen.findByRole("button", { name: /inbox\.actions\.archive/ }));
    await waitFor(() => expect(svc.archiveNotification).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /inbox\.actions\.archive/ })).toBeEnabled(),
    );
  });

  it("shows the empty label of the selected view", async () => {
    svc.listNotifications.mockResolvedValue([]);
    renderInbox();
    expect(await screen.findByText("inbox.empty.inbox")).toBeInTheDocument();
  });

  it("shows an error state with retry when the list cannot load", async () => {
    svc.listNotifications.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "x" });
    renderInbox();
    expect(await screen.findByText("inbox.loadError")).toBeInTheDocument();
  });
});

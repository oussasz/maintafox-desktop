import { render, screen } from "@testing-library/react";
import type { TFunction } from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationPreferencesPanel } from "@/components/notifications/NotificationPreferencesPanel";
import { composeOsToast, pollOsToasts } from "@/hooks/use-os-toasts";

const t = ((key: string, o?: Record<string, unknown>) =>
  o ? `${key}:${JSON.stringify(o)}` : key) as unknown as TFunction<"notifications">;

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t, i18n: { language: "en" } }),
}));

const svc = vi.hoisted(() => ({
  listPendingOsToasts: vi.fn(),
  showOsToast: vi.fn(),
  getNotificationPreferences: vi.fn(),
  getNotificationChannelStatus: vi.fn(),
  updateNotificationPreference: vi.fn(),
}));
vi.mock("@/services/notification-service", () => svc);

const item = (id: number, title: string) => ({
  notification_id: id,
  title,
  body: `body ${id}`,
  severity: "info",
  category_code: "wo_assigned",
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe("OS toasts", () => {
  it("shows a single notification as it is and several as a summary", () => {
    expect(composeOsToast([], t)).toBeNull();
    expect(composeOsToast([item(1, "OT-1 assigné")], t)).toEqual({
      title: "OT-1 assigné",
      body: "body 1",
    });
    expect(composeOsToast([item(1, "OT-1"), item(2, "OT-2"), item(3, "OT-3")], t)).toEqual({
      title: 'osToast.summaryTitle:{"count":3}',
      body: 'osToast.summaryBody:{"count":3,"first":"OT-1","rest":2}',
    });
  });

  it("takes a watermark first, then toasts what arrived since", async () => {
    svc.listPendingOsToasts.mockResolvedValueOnce({ watermark: 40, items: [] });
    expect(await pollOsToasts(null, t)).toBe(40);
    expect(svc.showOsToast).not.toHaveBeenCalled();

    svc.listPendingOsToasts.mockResolvedValueOnce({
      watermark: 40,
      items: [item(41, "OT-41"), item(42, "OT-42")],
    });
    svc.showOsToast.mockResolvedValueOnce({ shown: true, error: null });
    expect(await pollOsToasts(40, t)).toBe(40);
    expect(svc.listPendingOsToasts).toHaveBeenLastCalledWith(40);
    expect(svc.showOsToast).toHaveBeenCalledWith({
      notification_ids: [41, 42],
      title: 'osToast.summaryTitle:{"count":2}',
      body: 'osToast.summaryBody:{"count":2,"first":"OT-41","rest":1}',
    });
  });
});

describe("NotificationPreferencesPanel channels", () => {
  const row = {
    category_code: "wo_assigned",
    module_code: "ot",
    label: "WO Assigned",
    is_user_configurable: true,
    in_app_enabled: true,
    os_enabled: true,
    email_enabled: false,
    sms_enabled: false,
    digest_mode: "instant",
    muted_until: null,
  };

  const isDisabled = (el: HTMLElement) =>
    el.hasAttribute("disabled") || el.getAttribute("data-disabled") !== null;

  it("explains why email is not available and disables it", async () => {
    svc.getNotificationPreferences.mockResolvedValue([row]);
    svc.getNotificationChannelStatus.mockResolvedValue({
      email: { available: false, reason: "server_endpoint_not_enabled" },
    });
    render(<NotificationPreferencesPanel />);
    expect(
      await screen.findByText("preferences.emailUnavailable.server_endpoint_not_enabled"),
    ).toHaveAttribute("role", "status");
    expect(isDisabled(screen.getByRole("switch", { name: "preferences.col.email" }))).toBe(true);
    // No SMS channel exists: no SMS control is offered.
    expect(screen.queryByRole("switch", { name: /sms/i })).toBeNull();
  });

  it("enables email when the channel is available", async () => {
    svc.getNotificationPreferences.mockResolvedValue([row]);
    svc.getNotificationChannelStatus.mockResolvedValue({
      email: { available: true, reason: null },
    });
    render(<NotificationPreferencesPanel />);
    const email = await screen.findByRole("switch", { name: "preferences.col.email" });
    expect(isDisabled(email)).toBe(false);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("explains the delivery modes and keeps the daily digest itself instant", async () => {
    svc.getNotificationPreferences.mockResolvedValue([
      row,
      { ...row, category_code: "daily_digest", label: "Daily Digest" },
    ]);
    svc.getNotificationChannelStatus.mockResolvedValue({
      email: { available: true, reason: null },
    });
    render(<NotificationPreferencesPanel />);
    expect(await screen.findByText("preferences.digestHelp")).toBeTruthy();
    const ownRow = screen.getByRole("combobox", {
      name: /^preferences\.col\.digest category\.daily_digest/,
    });
    const otherRow = screen.getByRole("combobox", {
      name: /^preferences\.col\.digest category\.wo_assigned/,
    });
    expect(isDisabled(ownRow)).toBe(true);
    expect(isDisabled(otherRow)).toBe(false);
  });

  it("shows translated category names and disables the channels of a category turned off", async () => {
    svc.getNotificationPreferences.mockResolvedValue([{ ...row, in_app_enabled: false }]);
    svc.getNotificationChannelStatus.mockResolvedValue({
      email: { available: true, reason: null },
    });
    render(<NotificationPreferencesPanel />);
    expect(await screen.findByText(/^category\.wo_assigned/)).toBeTruthy();
    expect(screen.queryByText("WO Assigned")).toBeNull();
    expect(isDisabled(screen.getByRole("switch", { name: "preferences.col.email" }))).toBe(true);
    expect(isDisabled(screen.getByRole("switch", { name: /^preferences\.col\.os / }))).toBe(true);
  });
});

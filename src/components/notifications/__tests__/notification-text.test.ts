import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import {
  formatParam,
  localDayBounds,
  notificationReason,
  notificationText,
  tomorrowMorning,
} from "@/components/notifications/notification-text";
import { P } from "@shared/rbac/permissions.generated";

const t = ((key: string, o?: Record<string, unknown>) =>
  o ? `${key}:${JSON.stringify(o)}` : key) as unknown as TFunction<"notifications">;
const fmt = {
  formatDate: (v: unknown) => `D(${String(v)})`,
  formatDateTime: (v: unknown) => `DT(${String(v)})`,
  formatNumber: (n: number) => `N(${n})`,
};

describe("notification text", () => {
  it("renders a known message key with formatted parameters", () => {
    const text = notificationText(
      {
        title: "stored",
        body: null,
        message_key: "stock_critical",
        message_params: { code: "ART-1", title: "Bearing", available: 3, reorder_point: 5 },
      },
      t,
      fmt,
    );
    expect(text.title).toBe(
      'messages.stock_critical.title:{"code":"ART-1","title":"Bearing","available":"N(3)","reorder_point":"N(5)"}',
    );
  });

  it("keeps the stored text for unknown or missing keys", () => {
    expect(
      notificationText(
        { title: "Stored", body: "b", message_key: "future_key", message_params: null },
        t,
        fmt,
      ),
    ).toEqual({ title: "Stored", body: "b" });
  });

  it("lists the digest counts with category labels", () => {
    const text = notificationText(
      {
        title: "x",
        body: null,
        message_key: "daily_digest",
        message_params: { date: "2026-10-15", total: 3, counts: { wo_overdue: 2, pm_due: 1 } },
      },
      t,
      fmt,
    );
    expect(text.body).toContain("category.wo_overdue");
    expect(text.body).toContain("N(2)");
  });

  it("formats dates, date-times and missing values", () => {
    expect(formatParam("planned_end", "2026-10-15T10:00:00Z", fmt)).toBe(
      "DT(2026-10-15T10:00:00Z)",
    );
    expect(formatParam("expires_on", "2026-10-15", fmt)).toBe("D(2026-10-15)");
    expect(formatParam("code", null, fmt)).toBe("—");
  });

  it("explains permission and escalation reasons", () => {
    expect(notificationReason({ reason_code: "permission", reason_ref: P.INV_MANAGE }, t)).toBe(
      `reason.permission:{"permission":"${P.INV_MANAGE}"}`,
    );
    expect(notificationReason({ reason_code: "escalation", reason_ref: "2:60" }, t)).toBe(
      'reason.escalation:{"level":"2","minutes":"60"}',
    );
    expect(notificationReason({ reason_code: null, reason_ref: null }, t)).toBeNull();
  });

  it("computes local day bounds and tomorrow morning", () => {
    const start = new Date(localDayBounds("2026-10-15", false) as string);
    expect(start.getHours()).toBe(0);
    expect(start.getDate()).toBe(15);
    const end = new Date(localDayBounds("2026-10-15", true) as string);
    expect(end.getHours()).toBe(23);
    expect(localDayBounds("bad", false)).toBeUndefined();
    const m = tomorrowMorning(new Date(2026, 9, 15, 17, 30), 8);
    expect([m.getDate(), m.getHours(), m.getMinutes()]).toEqual([16, 8, 0]);
  });
});

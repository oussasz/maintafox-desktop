import { describe, expect, it } from "vitest";

import { resolveNotificationLink } from "@/components/notifications/notification-links";

describe("resolveNotificationLink", () => {
  it("maps the SLA breach form ?di= to the param the Requests page reads", () => {
    expect(resolveNotificationLink("/requests?di=42")).toEqual({
      path: "/requests?openDi=42",
      exact: true,
    });
  });

  it("keeps the current DI form and accepts the path form", () => {
    expect(resolveNotificationLink("/requests?openDi=7")?.path).toBe("/requests?openDi=7");
    expect(resolveNotificationLink("/requests/7")?.path).toBe("/requests?openDi=7");
  });

  it("opens work orders through ?openWo=", () => {
    expect(resolveNotificationLink("/work-orders/15")?.path).toBe("/work-orders?openWo=15");
    expect(resolveNotificationLink("/wo/1")?.path).toBe("/work-orders?openWo=1");
    expect(resolveNotificationLink("/work-orders?openWo=3")?.path).toBe("/work-orders?openWo=3");
  });

  it("opens the PM occurrence or plan named by the link", () => {
    expect(resolveNotificationLink("/pm?occurrence=9")).toEqual({
      path: "/pm?occurrence=9",
      exact: true,
    });
    expect(resolveNotificationLink("/pm?occurrence=abc")).toEqual({ path: "/pm", exact: false });
    expect(resolveNotificationLink("/pm")).toEqual({ path: "/pm", exact: false });
    expect(resolveNotificationLink("/pm?planId=4")).toEqual({
      path: "/pm?planId=4",
      exact: true,
    });
  });

  it("maps the old finance budget route", () => {
    expect(resolveNotificationLink("/finance/budget")?.path).toBe("/budget");
  });

  it("keeps other in-app routes as they are", () => {
    expect(resolveNotificationLink("/planning")).toEqual({ path: "/planning", exact: true });
    expect(resolveNotificationLink("/personnel")?.path).toBe("/personnel");
    expect(resolveNotificationLink("/settings?tab=notifications")?.path).toBe(
      "/settings?tab=notifications",
    );
  });

  it("falls back to the module when the id is not a number", () => {
    expect(resolveNotificationLink("/requests?di=abc")).toEqual({
      path: "/requests",
      exact: false,
    });
  });

  it("refuses missing and non in-app links", () => {
    expect(resolveNotificationLink(null)).toBeNull();
    expect(resolveNotificationLink("")).toBeNull();
    expect(resolveNotificationLink("https://example.com/x")).toBeNull();
    expect(resolveNotificationLink("//evil.example/x")).toBeNull();
    expect(resolveNotificationLink("javascript:alert(1)")).toBeNull();
  });
});

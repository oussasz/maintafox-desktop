import { describe, expect, it } from "vitest";

import {
  dueWindowParam,
  idParam,
  listLinks,
  scopeParam,
  textParam,
  withoutParams,
} from "../list-links";

describe("list links", () => {
  it("builds explicit, readable list filters", () => {
    expect(listLinks.workOrders({ scope: "open" })).toBe("/work-orders?scope=open");
    expect(listLinks.workOrders({ status: "in_progress", equipmentId: 12 })).toBe(
      "/work-orders?status=in_progress&equipment=12",
    );
    expect(listLinks.requests({ queue: "to_review", scope: "open" })).toBe(
      "/requests?queue=to_review&scope=open",
    );
    expect(listLinks.pmDue("overdue")).toBe("/pm?due=overdue");
    expect(listLinks.inventory({ belowReorder: true })).toBe("/inventory?stock=below_reorder");
    expect(listLinks.equipment()).toBe("/equipment");
  });

  it("reads only valid values", () => {
    expect(idParam("42")).toBe(42);
    expect(idParam("0")).toBeNull();
    expect(idParam("x")).toBeNull();
    expect(idParam(null)).toBeNull();
    expect(textParam("  ")).toBeNull();
    expect(textParam(" draft ")).toBe("draft");
    expect(scopeParam("open")).toBe("open");
    expect(scopeParam("all")).toBeNull();
    expect(dueWindowParam("next7")).toBe("next7");
    expect(dueWindowParam("later")).toBeNull();
  });

  it("removes a cleared filter from the address and keeps the rest", () => {
    const next = withoutParams(
      new URLSearchParams("queue=mine&status=x&scope=open"),
      "status",
      "scope",
    );
    expect(next.toString()).toBe("queue=mine");
  });
});

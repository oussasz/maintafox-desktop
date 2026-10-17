import { describe, expect, it } from "vitest";

import {
  dueWindowParam,
  idParam,
  listLinks,
  mixParam,
  natureInMix,
  scopeParam,
  textParam,
  waitingDaysParam,
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
    expect(listLinks.workOrders({ scope: "overdue" })).toBe("/work-orders?scope=overdue");
    expect(listLinks.requests({ waitingDays: 7 })).toBe("/requests?waiting=7");
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
    expect(scopeParam("overdue")).toBe("overdue");
    expect(scopeParam("all")).toBeNull();
    expect(waitingDaysParam("7")).toBe(7);
    expect(waitingDaysParam("0")).toBeNull();
    expect(waitingDaysParam("99999")).toBeNull();
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

describe("work mix", () => {
  it("links a work mix and reads it back", () => {
    expect(listLinks.workOrders({ scope: "open", mix: "emergency" })).toBe(
      "/work-orders?scope=open&mix=emergency",
    );
    expect(mixParam("reactive")).toBe("reactive");
    expect(mixParam("other")).toBeNull();
  });

  it("groups natures like the backend (planned / reactive / emergency)", () => {
    expect(natureInMix("corrective_immediate", "emergency")).toBe(true);
    expect(natureInMix("corrective_deferred", "emergency")).toBe(false);
    expect(natureInMix("corrective_deferred", "reactive")).toBe(true);
    expect(natureInMix("corrective_immediate", "reactive")).toBe(true);
    expect(natureInMix("preventive_predictive", "planned")).toBe(true);
    expect(natureInMix("improvement", "planned")).toBe(true);
    expect(natureInMix("improvement", "reactive")).toBe(false);
    expect(natureInMix(null, "planned")).toBe(false);
  });
});

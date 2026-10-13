import { describe, expect, it } from "vitest";

import { isStepUpRequiredError } from "@/utils/errors";

import { createLatestRequestGuard } from "../latest-request";

describe("createLatestRequestGuard", () => {
  it("only the latest request of a key stays current", () => {
    const guard = createLatestRequestGuard();
    const first = guard.begin("a");
    expect(first()).toBe(true);
    const second = guard.begin("a");
    expect(first()).toBe(false);
    expect(second()).toBe(true);
  });

  it("keys are independent and invalidate makes pending requests stale", () => {
    const guard = createLatestRequestGuard();
    const a = guard.begin("a");
    const b = guard.begin("b");
    guard.invalidate("a");
    expect(a()).toBe(false);
    expect(b()).toBe(true);
    const def = guard.begin();
    guard.begin("a");
    expect(def()).toBe(true);
  });
});

describe("isStepUpRequiredError", () => {
  it("recognises every shape a step-up failure can take", () => {
    expect(isStepUpRequiredError({ code: "STEP_UP_REQUIRED", message: "Re-authenticate" })).toBe(
      true,
    );
    expect(isStepUpRequiredError(new Error("STEP_UP_REQUIRED"))).toBe(true);
    expect(isStepUpRequiredError("STEP_UP_REQUIRED")).toBe(true);
    expect(isStepUpRequiredError({ kind: "StepUpRequired" })).toBe(true);
  });

  it("does not mistake other failures for a step-up", () => {
    expect(isStepUpRequiredError(new Error("timeout"))).toBe(false);
    expect(isStepUpRequiredError({ code: "VALIDATION", message: "cycle" })).toBe(false);
    expect(isStepUpRequiredError(null)).toBe(false);
  });
});

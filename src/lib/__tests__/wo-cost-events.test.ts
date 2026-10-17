import { describe, expect, it, vi } from "vitest";

import { notifyWoCostChanged, onWoCostChanged } from "../wo-cost-events";

describe("wo cost events", () => {
  it("calls only the listeners of the work order whose cost changed", () => {
    const a = vi.fn();
    const b = vi.fn();
    const offA = onWoCostChanged(1, a);
    const offB = onWoCostChanged(2, b);
    notifyWoCostChanged(1);
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).not.toHaveBeenCalled();
    offA();
    notifyWoCostChanged(1);
    expect(a).toHaveBeenCalledTimes(1);
    offB();
  });
});

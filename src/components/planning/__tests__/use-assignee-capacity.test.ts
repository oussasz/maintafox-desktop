import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { listAssigneeCapacity } from "@/services/planning-service";
import type { AssigneeCapacity } from "@shared/ipc-types";

import { useAssigneeCapacity } from "../use-assignee-capacity";

vi.mock("@/services/planning-service", () => ({
  listAssigneeCapacity: vi.fn(),
}));

const mocked = vi.mocked(listAssigneeCapacity);

function row(personnel_id: number): AssigneeCapacity {
  return {
    personnel_id,
    full_name: `Tech ${personnel_id}`,
    employee_code: null,
    days: [],
    slot_working_hours: 8,
    issues: [],
    unknown_hours_jobs: 0,
    unreadable_work_orders: [],
  };
}

const query = {
  enabled: true,
  teamId: 3 as number | null,
  startIso: "2026-05-04T08:00:00.000Z",
  endIso: "2026-05-04T10:00:00.000Z",
  candidateId: 11,
};

describe("useAssigneeCapacity", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocked.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("asks nothing while disabled or without a valid slot", async () => {
    const { result, rerender } = renderHook((props) => useAssigneeCapacity(props), {
      initialProps: { ...query, enabled: false },
    });
    rerender({ ...query, endIso: query.startIso });
    rerender({ ...query, teamId: null });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(mocked).not.toHaveBeenCalled();
    expect(result.current.size).toBe(0);
  });

  it("waits while the slot is being typed, then loads once", async () => {
    mocked.mockResolvedValue([row(7), row(8)]);
    const { result, rerender } = renderHook((props) => useAssigneeCapacity(props), {
      initialProps: query,
    });
    rerender({ ...query, endIso: "2026-05-04T11:00:00.000Z" });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(mocked).toHaveBeenCalledTimes(1);
    expect(mocked).toHaveBeenCalledWith({
      team_id: 3,
      start: query.startIso,
      end: "2026-05-04T11:00:00.000Z",
      candidate_id: 11,
      commitment_id: null,
    });
    expect([...result.current.keys()]).toEqual([7, 8]);
  });

  it("keeps the previous figures while the next ones load, and clears them on a failure", async () => {
    mocked.mockResolvedValueOnce([row(7)]);
    const { result, rerender } = renderHook((props) => useAssigneeCapacity(props), {
      initialProps: query,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(result.current.has(7)).toBe(true);

    mocked.mockRejectedValueOnce(new Error("boom"));
    rerender({ ...query, endIso: "2026-05-04T12:00:00.000Z" });
    expect(result.current.has(7)).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(result.current.size).toBe(0);
  });
});

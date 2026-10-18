/**
 * PM calendar reads: only the latest request sets the calendar's error and loading state. A month
 * the user already left that fails late must not show an error over the month now displayed, and
 * its end must not clear the loading state of the read still running.
 */
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { usePmCalendarLoad } from "@/components/pm/use-pm-calendar-load";
import type { PmCalendarRange } from "@/stores/pm-store";

const OCTOBER: PmCalendarRange = { from: "2026-09-27T00:00:00Z", to: "2026-11-09T23:59:59Z" };
const NOVEMBER: PmCalendarRange = { from: "2026-10-25T00:00:00Z", to: "2026-12-07T23:59:59Z" };

function deferred() {
  let resolve!: () => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("usePmCalendarLoad", () => {
  it("ignores a late failure of a month the user already left", async () => {
    const october = deferred();
    const november = deferred();
    const load = vi
      .fn<(range?: PmCalendarRange) => Promise<void>>()
      .mockReturnValueOnce(october.promise)
      .mockReturnValueOnce(november.promise);
    const { result } = renderHook(() => usePmCalendarLoad(load));

    act(() => result.current.loadRange(OCTOBER));
    act(() => result.current.loadRange(NOVEMBER));

    await act(async () => {
      october.reject({ code: "DATABASE_ERROR", message: "busy" });
      await october.promise.catch(() => undefined);
    });
    // The stale failure shows no error and does not end November's loading.
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(true);

    await act(async () => {
      november.resolve();
      await november.promise;
    });
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
  });

  it("ignores a late success of an earlier month while the latest read runs", async () => {
    const october = deferred();
    const november = deferred();
    const load = vi
      .fn<(range?: PmCalendarRange) => Promise<void>>()
      .mockReturnValueOnce(october.promise)
      .mockReturnValueOnce(november.promise);
    const { result } = renderHook(() => usePmCalendarLoad(load));

    act(() => result.current.loadRange(OCTOBER));
    act(() => result.current.loadRange(NOVEMBER));
    await act(async () => {
      october.resolve();
      await october.promise;
    });
    expect(result.current.loading).toBe(true);

    await act(async () => {
      november.resolve();
      await november.promise;
    });
    expect(result.current.loading).toBe(false);
  });

  it("shows the failure of the latest read, and a retry clears it", async () => {
    const failure = { code: "DATABASE_ERROR", message: "busy" };
    const load = vi
      .fn<(range?: PmCalendarRange) => Promise<void>>()
      .mockRejectedValueOnce(failure)
      .mockResolvedValueOnce(undefined);
    const { result } = renderHook(() => usePmCalendarLoad(load));

    await act(async () => {
      result.current.loadRange(NOVEMBER);
      await Promise.resolve();
    });
    expect(result.current.error).toBe(failure);
    expect(result.current.loading).toBe(false);

    await act(async () => {
      result.current.loadRange();
      await Promise.resolve();
    });
    expect(load).toHaveBeenLastCalledWith(undefined);
    expect(result.current.error).toBeNull();
    expect(result.current.loading).toBe(false);
  });
});

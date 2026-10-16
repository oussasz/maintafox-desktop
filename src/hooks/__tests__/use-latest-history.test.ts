import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useLatestHistory } from "@/hooks/use-latest-history";

/** Backend stand-in: the `limit` latest of `total` events, oldest first. */
function backend(total: number) {
  const all = Array.from({ length: total }, (_, i) => i + 1);
  return vi.fn(async (limit: number) => all.slice(Math.max(0, total - limit)));
}

describe("useLatestHistory", () => {
  it("shows everything and no notice when the history fits the page", async () => {
    const fetch = backend(3);
    const { result } = renderHook(() => useLatestHistory(fetch, { pageSize: 5, maxLimit: 20 }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([1, 2, 3]);
    expect(result.current.hasOlder).toBe(false);
    expect(fetch).toHaveBeenCalledWith(6);
  });

  it("keeps the latest page, flags older events and widens on load older", async () => {
    const fetch = backend(12);
    const { result } = renderHook(() => useLatestHistory(fetch, { pageSize: 5, maxLimit: 20 }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toEqual([8, 9, 10, 11, 12]);
    expect(result.current.hasOlder).toBe(true);
    expect(result.current.canLoadOlder).toBe(true);

    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.items).toHaveLength(10));
    expect(result.current.items[0]).toBe(3);
    expect(result.current.loading).toBe(false);
    expect(result.current.loadingOlder).toBe(false);

    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.items).toHaveLength(12));
    expect(result.current.hasOlder).toBe(false);
    act(() => result.current.loadOlder());
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("keeps the notice at the backend maximum without offering more", async () => {
    const fetch = backend(50);
    const { result } = renderHook(() => useLatestHistory(fetch, { pageSize: 10, maxLimit: 10 }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.items).toHaveLength(10);
    expect(result.current.hasOlder).toBe(true);
    expect(result.current.canLoadOlder).toBe(false);
  });

  it("reports a failure (no empty history) and retries", async () => {
    const fetch = vi
      .fn<(limit: number) => Promise<number[]>>()
      .mockRejectedValueOnce({ code: "DATABASE_ERROR" })
      .mockResolvedValueOnce([1]);
    const { result } = renderHook(() => useLatestHistory(fetch, { pageSize: 5, maxLimit: 20 }));
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error).toEqual({ code: "DATABASE_ERROR" });
    expect(result.current.items).toEqual([]);

    act(() => result.current.reload());
    await waitFor(() => expect(result.current.items).toEqual([1]));
    expect(result.current.error).toBeNull();
  });

  it("starts again from the first page for another record", async () => {
    const a = backend(12);
    const b = backend(2);
    const { result, rerender } = renderHook(
      ({ f }) => useLatestHistory(f, { pageSize: 5, maxLimit: 20 }),
      { initialProps: { f: a } },
    );
    await waitFor(() => expect(result.current.loading).toBe(false));
    act(() => result.current.loadOlder());
    await waitFor(() => expect(result.current.items).toHaveLength(10));

    rerender({ f: b });
    await waitFor(() => expect(result.current.items).toEqual([1, 2]));
    expect(b).toHaveBeenCalledWith(6);
  });

  it("ignores a response that arrives after the record changed", async () => {
    let resolveSlow: (rows: number[]) => void = () => {};
    const slow = vi.fn(
      () =>
        new Promise<number[]>((resolve) => {
          resolveSlow = resolve;
        }),
    );
    const failSlow = vi.fn(() => Promise.reject(new Error("late")));
    const fast = backend(1);
    const { result, rerender } = renderHook(
      ({ f }) => useLatestHistory(f, { pageSize: 5, maxLimit: 20 }),
      { initialProps: { f: slow as (limit: number) => Promise<number[]> } },
    );
    rerender({ f: fast });
    await waitFor(() => expect(result.current.items).toEqual([1]));
    await act(async () => resolveSlow([9, 9, 9]));
    expect(result.current.items).toEqual([1]);

    rerender({ f: failSlow });
    rerender({ f: fast });
    await waitFor(() => expect(result.current.items).toEqual([1]));
    expect(result.current.error).toBeNull();
  });
});

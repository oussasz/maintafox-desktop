/**
 * L6-18: the return / rejection dialogs open over the request sheet; once one is gone the sheet
 * re-reads the request.
 */
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useRefreshAfterQuestion } from "@/components/di/use-refresh-after-question";
import { useDiReviewStore } from "@/stores/di-review-store";
import type { InterventionRequest } from "@shared/ipc-types";

vi.mock("@/lib/ipc-invoke", () => ({ invoke: vi.fn() }));

const di = { id: 5 } as InterventionRequest;

describe("useRefreshAfterQuestion", () => {
  beforeEach(() => {
    useDiReviewStore.setState({ returnDi_: null, rejectionDi: null });
  });

  it("reloads once when a small dialog closes over an open sheet", () => {
    const reload = vi.fn();
    const { result } = renderHook(() => useRefreshAfterQuestion(true, reload));
    expect(result.current).toBe(false);

    act(() => useDiReviewStore.getState().openRejection(di));
    expect(result.current).toBe(true);
    expect(reload).not.toHaveBeenCalled();

    act(() => useDiReviewStore.getState().closeRejection());
    expect(result.current).toBe(false);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("also covers the return-for-clarification dialog", () => {
    const reload = vi.fn();
    renderHook(() => useRefreshAfterQuestion(true, reload));
    act(() => useDiReviewStore.getState().openReturn(di));
    act(() => useDiReviewStore.getState().closeReturn());
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("does nothing when the sheet itself is closed", () => {
    const reload = vi.fn();
    renderHook(() => useRefreshAfterQuestion(false, reload));
    act(() => useDiReviewStore.getState().openRejection(di));
    act(() => useDiReviewStore.getState().closeRejection());
    expect(reload).not.toHaveBeenCalled();
  });
});

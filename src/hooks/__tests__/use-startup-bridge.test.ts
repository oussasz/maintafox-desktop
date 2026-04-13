import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";

import { useStartupBridge } from "@/hooks/use-startup-bridge";
import { useAppStore } from "@/store/app-store";
import { mockInvoke, fixtures } from "@/test/mocks/tauri";
import type { StartupEvent } from "@shared/ipc-types";

// ── Mock @tauri-apps/api/event ────────────────────────────────────────────
type EventCallback = (event: { payload: StartupEvent }) => void;
let capturedListener: EventCallback | null = null;
const mockUnlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_eventName: string, callback: EventCallback) => {
    capturedListener = callback;
    return Promise.resolve(mockUnlisten);
  }),
}));

function emitEvent(payload: StartupEvent) {
  capturedListener?.({ payload });
}

// ─────────────────────────────────────────────────────────────────────────────
describe("useStartupBridge", () => {
  beforeEach(() => {
    capturedListener = null;
    mockUnlisten.mockClear();
    mockInvoke.mockReset();
    // Default: getAppInfo returns a valid fixture so Zod parse succeeds
    mockInvoke.mockResolvedValue(fixtures.appInfo);
    // Reset store to initial loading state
    useAppStore.setState({
      appStatus: "loading",
      startupMessage: "",
      appVersion: "",
    });
  });

  it("transitions app status to ready when stage is ready", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({ stage: "ready" });

    expect(useAppStore.getState().appStatus).toBe("ready");
  });

  it("transitions app status to error when stage is failed", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({ stage: "failed", reason: "DB corrupt" });

    const { appStatus, startupMessage } = useAppStore.getState();
    expect(appStatus).toBe("error");
    expect(startupMessage).toBe("DB corrupt");
  });

  it("updates startup message on db_ready stage", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({ stage: "db_ready" });

    const { appStatus, startupMessage } = useAppStore.getState();
    expect(appStatus).toBe("loading");
    expect(startupMessage).toContain("Base de donn");
  });

  it("updates startup message on migrations_complete stage", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({ stage: "migrations_complete", applied: 3 });

    const { appStatus, startupMessage } = useAppStore.getState();
    expect(appStatus).toBe("loading");
    expect(startupMessage).toContain("Migrations");
  });

  it("updates startup message on entitlement_cache_loaded stage", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({ stage: "entitlement_cache_loaded" });

    const { appStatus, startupMessage } = useAppStore.getState();
    expect(appStatus).toBe("loading");
    expect(startupMessage).toContain("Configuration");
  });

  it("fetches app version via getAppInfo during bootstrap", async () => {
    renderHook(() => useStartupBridge());

    await waitFor(() => {
      expect(useAppStore.getState().appVersion).toBe("0.1.0");
    });
  });

  it("cleans up listener on unmount", async () => {
    const { unmount } = renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    unmount();

    expect(mockUnlisten).toHaveBeenCalled();
  });
});

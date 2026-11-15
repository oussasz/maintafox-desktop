import { renderHook, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

import { STARTUP_STALL_AFTER_MS, useStartupBridge } from "@/hooks/use-startup-bridge";
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
      startupStalled: false,
      appVersion: "",
      updateRecovery: null,
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
  it("shows the update recovery when update_rollback_required is emitted", async () => {
    renderHook(() => useStartupBridge());
    await waitFor(() => expect(capturedListener).not.toBeNull());

    emitEvent({
      stage: "update_rollback_required",
      from_version: "1.1.0",
      to_version: "1.2.0",
      reason: "Migration failed: m20261001 added column twice",
      backup_refusal: null,
    });

    const { appStatus, updateRecovery } = useAppStore.getState();
    expect(appStatus).toBe("update_rollback_required");
    expect(updateRecovery).toEqual({
      fromVersion: "1.1.0",
      toVersion: "1.2.0",
      reason: "Migration failed: m20261001 added column twice",
      backupRefusal: null,
    });
  });

  it("recovers a missed update_rollback_required from the journal, never saying ready", async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === "get_app_info") return { ...fixtures.appInfo, version: "1.2.0" };
      if (command === "get_update_journal") {
        return {
          state: "rollback_required",
          kind: "upgrade",
          from_version: "1.1.0",
          to_version: "1.2.0",
          failure: "Migration failed",
          rollback_refusal: null,
          rollback_allowed_version: null,
          backup_refusal: "the backup changed since it was taken (checksum mismatch)",
          notice_acknowledged: false,
        };
      }
      if (command === "health_check") return fixtures.healthCheck;
      return null;
    });

    renderHook(() => useStartupBridge());

    await waitFor(() => {
      expect(useAppStore.getState().appStatus).toBe("update_rollback_required");
    });
    // Give the fallback probes time to run: they must not override the recovery status.
    await new Promise((resolve) => setTimeout(resolve, 50));
    await Promise.resolve();
    expect(useAppStore.getState().appStatus).toBe("update_rollback_required");
    expect(useAppStore.getState().updateRecovery).toEqual({
      fromVersion: "1.1.0",
      toVersion: "1.2.0",
      reason: "Migration failed",
      backupRefusal: "the backup changed since it was taken (checksum mismatch)",
    });
  });

  it("ignores a rollback_required journal that belongs to another version", async () => {
    mockInvoke.mockImplementation(async (command) => {
      if (command === "get_app_info") return { ...fixtures.appInfo, version: "1.1.0" };
      if (command === "get_update_journal") {
        return {
          state: "rollback_required",
          kind: "upgrade",
          from_version: "1.1.0",
          to_version: "1.2.0",
          failure: null,
          rollback_refusal: null,
          rollback_allowed_version: null,
          notice_acknowledged: false,
        };
      }
      if (command === "health_check") return fixtures.healthCheck;
      return null;
    });

    renderHook(() => useStartupBridge());

    await waitFor(() => expect(useAppStore.getState().appStatus).toBe("ready"));
    expect(useAppStore.getState().updateRecovery).toBeNull();
  });

  describe("backend that is slow to answer", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it("applies the backend's last start-up event when ready was emitted before the listener", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "get_app_info") return fixtures.appInfo;
        if (command === "get_last_startup_event") return { stage: "ready" };
        if (command === "health_check") throw new Error("timeout");
        return null;
      });

      renderHook(() => useStartupBridge());
      await vi.advanceTimersByTimeAsync(10);

      expect(useAppStore.getState().appStatus).toBe("ready");
    });

    it("keeps asking the health check until the busy database answers", async () => {
      let healthCalls = 0;
      mockInvoke.mockImplementation(async (command) => {
        if (command === "get_app_info") return fixtures.appInfo;
        if (command === "health_check") {
          healthCalls += 1;
          if (healthCalls < 4) throw new Error("timeout");
          return fixtures.healthCheck;
        }
        return null;
      });

      renderHook(() => useStartupBridge());
      await vi.advanceTimersByTimeAsync(10);
      expect(useAppStore.getState().appStatus).toBe("loading");

      await vi.advanceTimersByTimeAsync(10_000);

      expect(healthCalls).toBe(4);
      expect(useAppStore.getState().appStatus).toBe("ready");
    });

    it("flags a stalled start after a while, and a late answer still gets in", async () => {
      let healthy = false;
      mockInvoke.mockImplementation(async (command) => {
        if (command === "get_app_info") return fixtures.appInfo;
        if (command === "health_check") {
          if (!healthy) throw new Error("timeout");
          return fixtures.healthCheck;
        }
        return null;
      });

      renderHook(() => useStartupBridge());
      await vi.advanceTimersByTimeAsync(STARTUP_STALL_AFTER_MS - 1000);
      expect(useAppStore.getState().startupStalled).toBe(false);

      await vi.advanceTimersByTimeAsync(2000);
      expect(useAppStore.getState().startupStalled).toBe(true);
      expect(useAppStore.getState().appStatus).toBe("loading");

      healthy = true;
      await vi.advanceTimersByTimeAsync(5000);
      expect(useAppStore.getState().appStatus).toBe("ready");
    });

    it("stops asking once the screen is unmounted", async () => {
      mockInvoke.mockImplementation(async (command) => {
        if (command === "get_app_info") return fixtures.appInfo;
        throw new Error("timeout");
      });

      const { unmount } = renderHook(() => useStartupBridge());
      await vi.advanceTimersByTimeAsync(10);
      unmount();
      const callsAtUnmount = mockInvoke.mock.calls.length;
      await vi.advanceTimersByTimeAsync(20_000);

      expect(mockInvoke.mock.calls.length).toBe(callsAtUnmount);
    });
  });
});

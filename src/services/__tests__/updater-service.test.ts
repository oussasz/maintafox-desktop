/**
 * updater-service.test.ts
 *
 * Tests for the updater service and the updater Zustand store.
 * The @tauri-apps/api/core invoke mock is set up globally in src/test/setup.ts.
 *
 * Coverage goals (SP06-F02 S3):
 *   1. Zod schema accepts and rejects expected shapes.
 *   2. checkForUpdate() resolves correctly for both available/unavailable cases.
 *   3. Store state transitions: isChecking, lastCheckResult, error.
 */

import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";

import { useUpdaterStore } from "@/stores/updater-store";
import { mockInvoke, fixtures } from "@/test/mocks/tauri";

import { checkForUpdate } from "../updater-service";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function resetStore(): void {
  useUpdaterStore.setState({
    lastCheckResult: null,
    isChecking: false,
    isInstalling: false,
    installComplete: false,
    error: null,
  });
}

// ─────────────────────────────────────────────────────────────────────────────
// checkForUpdate — service layer
// ─────────────────────────────────────────────────────────────────────────────

describe("checkForUpdate (service)", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("returns available=false when no update exists", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckNoUpdate);

    const result = await checkForUpdate();

    expect(result.available).toBe(false);
    expect(result.version).toBeNull();
    expect(result.notes).toBeNull();
    expect(result.pub_date).toBeNull();
  });

  it("returns update data when an update is available", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckAvailable);

    const result = await checkForUpdate();

    expect(result.available).toBe(true);
    expect(result.version).toBe("1.2.0");
    expect(typeof result.notes).toBe("string");
  });

  it("invokes the check_for_update IPC command", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckNoUpdate);

    await checkForUpdate();

    expect(mockInvoke).toHaveBeenCalledWith("check_for_update");
  });

  it("throws a ZodError when Rust returns a malformed response", async () => {
    // available must be boolean — number should fail Zod validation
    mockInvoke.mockResolvedValueOnce({ available: 1, version: null, notes: null, pub_date: null });

    await expect(checkForUpdate()).rejects.toThrow();
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// useUpdaterStore — state transitions
// ─────────────────────────────────────────────────────────────────────────────

describe("useUpdaterStore", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("isChecking transitions to false after a successful check", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckNoUpdate);

    const { result } = renderHook(() => useUpdaterStore());

    await act(async () => {
      await result.current.checkForUpdate();
    });

    expect(result.current.isChecking).toBe(false);
    expect(result.current.lastCheckResult?.available).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("lastCheckResult reflects available=true when update found", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.updateCheckAvailable);

    const { result } = renderHook(() => useUpdaterStore());

    await act(async () => {
      await result.current.checkForUpdate();
    });

    expect(result.current.lastCheckResult?.available).toBe(true);
    expect(result.current.lastCheckResult?.version).toBe("1.2.0");
  });

  it("error is set and isChecking is false when invoke throws", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("network unreachable"));

    const { result } = renderHook(() => useUpdaterStore());

    await act(async () => {
      await result.current.checkForUpdate();
    });

    expect(result.current.error).toContain("network unreachable");
    expect(result.current.isChecking).toBe(false);
    expect(result.current.lastCheckResult).toBeNull();
  });

  it("dismissNotification resets lastCheckResult, error, and installComplete", async () => {
    useUpdaterStore.setState({
      lastCheckResult: { available: true, version: "1.2.0", notes: null, pub_date: null },
      error: "some previous error",
      installComplete: true,
    });

    const { result } = renderHook(() => useUpdaterStore());

    act(() => {
      result.current.dismissNotification();
    });

    expect(result.current.lastCheckResult).toBeNull();
    expect(result.current.error).toBeNull();
    expect(result.current.installComplete).toBe(false);
  });
});

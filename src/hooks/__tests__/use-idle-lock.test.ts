/**
 * AU-34: the idle lock follows real user input, not IPC traffic. No input for the idle timeout →
 * the session is read again and the lock shows, even while polls run; input → `touch_session`
 * (throttled); no input → no activity report.
 */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSessionStore } from "@/store/session-store";
import type { SessionInfo } from "@shared/ipc-types";

import { useIdleLock } from "../use-idle-lock";

const svc = vi.hoisted(() => ({
  touchSession: vi.fn(),
  getSessionInfo: vi.fn(),
  confirmSessionAfterSilentFailure: vi.fn(async () => undefined),
}));

vi.mock("@/services/auth-service", () => ({
  touchSession: svc.touchSession,
  getSessionInfo: svc.getSessionInfo,
  login: vi.fn(),
  logout: vi.fn(),
  unlockSession: vi.fn(),
  forceChangePassword: vi.fn(),
}));

/** Events the test marks as made by the user (jsdom only dispatches untrusted events). */
const trusted = vi.hoisted(() => new WeakSet<Event>());
vi.mock("@/lib/trusted-input", () => ({ isTrustedInput: (e: Event) => trusted.has(e) }));

function userInput(event: Event): void {
  trusted.add(event);
  window.dispatchEvent(event);
}

vi.mock("@/lib/ipc-invoke", () => ({
  confirmSessionAfterSilentFailure: svc.confirmSessionAfterSilentFailure,
}));

const IDLE_MINUTES = 2;
const IDLE_MS = IDLE_MINUTES * 60_000;
/** A quarter of the 2-minute idle timeout. */
const THROTTLE_MS = 30_000;

function session(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    is_authenticated: true,
    is_locked: false,
    user_id: 7,
    username: "tech",
    display_name: "Technicien",
    is_admin: false,
    force_password_change: false,
    expires_at: new Date(Date.now() + 8 * 3_600_000).toISOString(),
    last_activity_at: new Date(Date.now()).toISOString(),
    idle_timeout_minutes: IDLE_MINUTES,
    password_expires_in_days: null,
    pin_configured: false,
    tenant_id: "t",
    token_tenant_id: "t",
    ...overrides,
  };
}

/** What the server says at the time of the call: locked once the idle deadline passed. */
function serverAnswer(lastActivityMs: () => number) {
  return async () => {
    const locked = Date.now() > lastActivityMs() + IDLE_MS;
    return session({
      last_activity_at: new Date(lastActivityMs()).toISOString(),
      is_authenticated: !locked,
      is_locked: locked,
    });
  };
}

describe("useIdleLock", () => {
  let serverLastActivity = 0;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-22T08:00:00Z"));
    svc.touchSession.mockReset();
    svc.getSessionInfo.mockReset();
    svc.confirmSessionAfterSilentFailure.mockClear();
    serverLastActivity = Date.now();
    svc.touchSession.mockImplementation(async () => {
      serverLastActivity = Date.now();
      return session({ last_activity_at: new Date(serverLastActivity).toISOString() });
    });
    svc.getSessionInfo.mockImplementation(serverAnswer(() => serverLastActivity));
    useSessionStore.getState().resetForTests();
    useSessionStore.getState().applySessionInfo(session());
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the lock after the idle timeout without input, even while a poll runs", async () => {
    // A background poll (notifications, presence) runs every 30 s and touches nothing.
    const poll = vi.fn();
    const pollId = window.setInterval(poll, 30_000);
    renderHook(() => useIdleLock());

    await act(async () => {
      await vi.advanceTimersByTimeAsync(IDLE_MS - 5_000);
    });
    expect(useSessionStore.getState().info?.is_locked).toBe(false);
    expect(svc.getSessionInfo).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(poll).toHaveBeenCalled();
    expect(svc.getSessionInfo).toHaveBeenCalledTimes(1);
    expect(svc.touchSession).not.toHaveBeenCalled();
    expect(useSessionStore.getState().info?.is_locked).toBe(true);
    window.clearInterval(pollId);
  });

  it("sends no activity report without input", async () => {
    useSessionStore.getState().applySessionInfo(session({ idle_timeout_minutes: 30 }));
    renderHook(() => useIdleLock());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(20 * 60_000);
    });
    expect(svc.touchSession).not.toHaveBeenCalled();
  });

  it("reports input at once, then at most once per interval", async () => {
    renderHook(() => useIdleLock());

    await act(async () => {
      userInput(new Event("pointermove"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(svc.touchSession).toHaveBeenCalledTimes(1);

    // Busy typing for 10 s: no further report inside the interval.
    await act(async () => {
      for (let i = 0; i < 10; i += 1) {
        userInput(new KeyboardEvent("keydown", { key: "a" }));
        await vi.advanceTimersByTimeAsync(1_000);
      }
    });
    expect(svc.touchSession).toHaveBeenCalledTimes(1);

    // The input made during the interval is reported at its end.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(THROTTLE_MS);
    });
    expect(svc.touchSession).toHaveBeenCalledTimes(2);

    // Then quiet: nothing more is sent.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(THROTTLE_MS * 2);
    });
    expect(svc.touchSession).toHaveBeenCalledTimes(2);
  });

  it("input keeps the session open past the first deadline", async () => {
    renderHook(() => useIdleLock());
    // Input every 50 s for 5 minutes (2.5 idle timeouts).
    for (let i = 0; i < 6; i += 1) {
      await act(async () => {
        userInput(new Event("pointerdown"));
        await vi.advanceTimersByTimeAsync(50_000);
      });
    }
    expect(useSessionStore.getState().info?.is_locked).toBe(false);
    expect(svc.touchSession.mock.calls.length).toBeGreaterThanOrEqual(5);

    // Then nothing: the lock shows one idle timeout after the last report.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(IDLE_MS + 2_000);
    });
    expect(useSessionStore.getState().info?.is_locked).toBe(true);
  });

  it("a refused report (session locked past its deadline) re-reads the session", async () => {
    const refusal = { code: "SESSION_LOCKED", message: "Session verrouillée" };
    svc.touchSession.mockRejectedValue(refusal);
    renderHook(() => useIdleLock());
    await act(async () => {
      userInput(new Event("pointerdown"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(svc.confirmSessionAfterSilentFailure).toHaveBeenCalledWith(refusal);
  });

  it("does nothing while the session is locked", async () => {
    useSessionStore
      .getState()
      .applySessionInfo(session({ is_authenticated: false, is_locked: true }));
    renderHook(() => useIdleLock());
    await act(async () => {
      userInput(new Event("pointerdown"));
      await vi.advanceTimersByTimeAsync(IDLE_MS * 2);
    });
    expect(svc.touchSession).not.toHaveBeenCalled();
    expect(svc.getSessionInfo).not.toHaveBeenCalled();
  });

  it("ignores events dispatched by a script", async () => {
    renderHook(() => useIdleLock());
    await act(async () => {
      window.dispatchEvent(new Event("pointerdown"));
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
      await vi.advanceTimersByTimeAsync(THROTTLE_MS * 2);
    });
    expect(svc.touchSession).not.toHaveBeenCalled();
    // The idle lock still comes on time.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(IDLE_MS);
    });
    expect(useSessionStore.getState().info?.is_locked).toBe(true);
  });
});

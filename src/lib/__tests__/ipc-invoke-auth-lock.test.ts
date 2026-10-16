import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/ipc-invoke";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { useSessionStore } from "@/store/session-store";
import { fixtures, mockInvoke } from "@/test/mocks/tauri";

describe("ipc-invoke auth lock gating", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    useSessionStore.getState().resetForTests();
    useAuthInterceptorStore.getState().clear();
  });

  it("opens AuthLock when AUTH_ERROR and silent refresh confirms unauthenticated", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "AUTH_ERROR",
        message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
      })
      .mockResolvedValueOnce(fixtures.noSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useAuthInterceptorStore.getState().isLockOpen).toBe(true);
      expect(useAuthInterceptorStore.getState().mode).toBe("session");
    });
    expect(useSessionStore.getState().info?.is_authenticated).toBe(false);
  });

  it("does not open AuthLock when AUTH_ERROR refresh shows idle-locked session", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "AUTH_ERROR",
        message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
      })
      .mockResolvedValueOnce(fixtures.lockedSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useSessionStore.getState().info?.is_locked).toBe(true);
    });
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });

  it("does not open AuthLock on SESSION_LOCKED when refresh shows locked session", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "SESSION_LOCKED",
        message:
          "Session locked: Session verrouillée pour inactivité. Veuillez vous déverrouiller.",
      })
      .mockResolvedValueOnce(fixtures.lockedSession);

    await expect(invoke("get_unread_count")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useSessionStore.getState().info?.is_locked).toBe(true);
    });
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });

  it("routes PASSWORD_CHANGE_REQUIRED to the change-password screen, without AuthLock", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "PASSWORD_CHANGE_REQUIRED",
        message: "PASSWORD_CHANGE_REQUIRED: le mot de passe doit être changé avant de continuer.",
      })
      .mockResolvedValueOnce(fixtures.authenticatedSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useSessionStore.getState().info?.force_password_change).toBe(true);
    });
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });

  it("takes the server state when the change is no longer required", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "VALIDATION_FAILED",
        message: "Le changement de mot de passe n'est plus requis.",
      })
      .mockResolvedValueOnce({ ...fixtures.authenticatedSession, force_password_change: false });

    await expect(useSessionStore.getState().changePassword("Nouveau#2026")).rejects.toThrow();
    expect(useSessionStore.getState().info?.force_password_change).toBe(false);
  });

  it("does not open AuthLock when AUTH_ERROR refresh still shows authenticated", async () => {
    mockInvoke
      .mockRejectedValueOnce({
        code: "AUTH_ERROR",
        message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
      })
      .mockResolvedValueOnce(fixtures.authenticatedSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useSessionStore.getState().info?.is_authenticated).toBe(true);
    });
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });
});

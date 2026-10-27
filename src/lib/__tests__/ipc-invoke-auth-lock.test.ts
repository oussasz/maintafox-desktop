import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { invoke } from "@/lib/ipc-invoke";
import { useAppToastStore } from "@/store/app-toast-store";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { useSessionStore } from "@/store/session-store";
import { fixtures, mockInvoke } from "@/test/mocks/tauri";

describe("ipc-invoke auth lock gating", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
  });

  beforeEach(() => {
    useAppToastStore.setState({ items: [] });
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

  // AU-35: a refused right is one toast with the translated reason; the app stays usable.
  const refused = {
    code: "PERMISSION_DENIED",
    message: "Permission denied: Permission requise : adm.users",
  };

  it("shows one translated toast for a refused right and never opens the lock", async () => {
    mockInvoke.mockRejectedValueOnce(refused);

    await expect(invoke("get_admin_stats")).rejects.toEqual(refused);

    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
    const toasts = useAppToastStore.getState().items;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({
      title: "Permission refusée.",
      description: "Droit requis : Gérer les comptes utilisateurs",
      variant: "destructive",
    });
    // The server's technical English text is never shown.
    expect(JSON.stringify(toasts[0])).not.toContain("Permission requise");
    // No session re-read either: a refused right is not a session problem.
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("gives a refused right only to the caller with notifyOnError:false (no toast, no lock)", async () => {
    mockInvoke.mockRejectedValueOnce(refused);

    await expect(invoke("get_admin_stats", undefined, { notifyOnError: false })).rejects.toEqual(
      refused,
    );

    expect(useAppToastStore.getState().items).toHaveLength(0);
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });

  // A password check inside the session (here: setting the PIN) locked the account; the server
  // ended the session. Sign-in shows the lockout message; no lock dialog, no toast, no re-read.
  it("sends the user to sign-in with the lockout message on ACCOUNT_LOCKED", async () => {
    useSessionStore.setState({
      info: {
        ...fixtures.authenticatedSession,
        password_expires_in_days: null,
        pin_configured: null,
      },
      hasBootstrapped: true,
    });
    const lockout = {
      code: "ACCOUNT_LOCKED",
      message: "Account locked until 2026-10-22T09:15:00Z",
    };
    mockInvoke.mockRejectedValueOnce(lockout);

    await expect(invoke("set_pin")).rejects.toEqual(lockout);

    await vi.waitFor(() => {
      expect(useSessionStore.getState().errorCode).toBe("ACCOUNT_LOCKED");
    });
    const state = useSessionStore.getState();
    expect(state.info?.is_authenticated).toBe(false);
    expect(state.error).toBe(lockout.message);
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
    expect(useAppToastStore.getState().items).toHaveLength(0);
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("still opens the lock, without a toast, for a lost session", async () => {
    mockInvoke
      .mockRejectedValueOnce({ code: "SESSION_CLAIM_INVALID", message: "Session claim invalid" })
      .mockResolvedValueOnce(fixtures.noSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await vi.waitFor(() => {
      expect(useAuthInterceptorStore.getState().isLockOpen).toBe(true);
    });
    expect(useAuthInterceptorStore.getState().mode).toBe("session");
    expect(useAppToastStore.getState().items).toHaveLength(0);
  });
});

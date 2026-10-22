/**
 * F1 (owner test of 0.9.1): after "Session expirée ou absente", the menu kept only the ungated
 * items and the dashboard its error, even after signing in again. The session generation is raised
 * whenever a session becomes usable, and once after an authentication race; permissions and the
 * dashboard reload on it.
 */
import { beforeEach, describe, expect, it } from "vitest";

import { confirmSessionAfterSilentFailure } from "@/lib/ipc-invoke";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { useSessionStore } from "@/store/session-store";
import { fixtures, mockInvoke } from "@/test/mocks/tauri";

const AUTH_ERROR = {
  code: "AUTH_ERROR",
  message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
};

describe("session generation", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    useSessionStore.getState().resetForTests();
    useAuthInterceptorStore.getState().clear();
  });

  it("is raised by a sign-in and by an unlock", async () => {
    mockInvoke.mockResolvedValueOnce({ session_info: fixtures.authenticatedSession });
    await useSessionStore.getState().login({ username: "admin", password: "x" });
    expect(useSessionStore.getState().sessionGeneration).toBe(1);

    mockInvoke.mockResolvedValueOnce(fixtures.authenticatedSession);
    await useSessionStore.getState().unlock("x");
    expect(useSessionStore.getState().sessionGeneration).toBe(2);
  });

  it("is raised when a refresh finds a usable session where there was none, not on every refresh", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.noSession);
    await useSessionStore.getState().refresh();
    expect(useSessionStore.getState().sessionGeneration).toBe(0);

    // A PIN unlock or a re-login elsewhere: the next refresh sees a usable session.
    mockInvoke.mockResolvedValueOnce(fixtures.authenticatedSession);
    await useSessionStore.getState().refresh();
    expect(useSessionStore.getState().sessionGeneration).toBe(1);

    mockInvoke.mockResolvedValueOnce(fixtures.authenticatedSession);
    await useSessionStore.getState().refresh();
    expect(useSessionStore.getState().sessionGeneration).toBe(1);
  });

  it("is raised once after an authentication race, never in a loop", async () => {
    mockInvoke.mockResolvedValueOnce({ session_info: fixtures.authenticatedSession });
    await useSessionStore.getState().login({ username: "admin", password: "x" });
    expect(useSessionStore.getState().sessionGeneration).toBe(1);

    // A silent call (the permission list) was refused, but the session is valid.
    mockInvoke.mockResolvedValue(fixtures.authenticatedSession);
    await confirmSessionAfterSilentFailure(AUTH_ERROR);
    expect(useSessionStore.getState().sessionGeneration).toBe(2);

    // Refused again before any new session: no second reload.
    await confirmSessionAfterSilentFailure(AUTH_ERROR);
    expect(useSessionStore.getState().sessionGeneration).toBe(2);
  });

  it("is not raised when the session is really lost, nor for other errors", async () => {
    mockInvoke.mockResolvedValueOnce({ session_info: fixtures.authenticatedSession });
    await useSessionStore.getState().login({ username: "admin", password: "x" });

    mockInvoke.mockResolvedValue(fixtures.noSession);
    await confirmSessionAfterSilentFailure(AUTH_ERROR);
    expect(useSessionStore.getState().info?.is_authenticated).toBe(false);
    expect(useSessionStore.getState().sessionGeneration).toBe(1);

    await confirmSessionAfterSilentFailure({ code: "PERMISSION_DENIED", message: "x" });
    expect(useSessionStore.getState().sessionGeneration).toBe(1);
  });
});

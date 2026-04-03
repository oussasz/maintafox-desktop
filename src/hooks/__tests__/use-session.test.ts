import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";

import { useSession } from "@/hooks/use-session";
import { mockInvoke, fixtures } from "@/test/mocks/tauri";

describe("useSession", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("fetches session info on mount", async () => {
    mockInvoke.mockResolvedValueOnce(fixtures.noSession);
    const { result } = renderHook(() => useSession());

    await act(async () => {});
    expect(mockInvoke).toHaveBeenCalledWith("get_session_info");
    expect(result.current.info?.is_authenticated).toBe(false);
  });

  it("login updates session info", async () => {
    mockInvoke
      .mockResolvedValueOnce(fixtures.noSession) // initial refresh
      .mockResolvedValueOnce({ session_info: fixtures.authenticatedSession }); // login response

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => {
      await result.current.login({ username: "admin", password: "Admin#2026!" });
    });

    expect(result.current.info?.is_authenticated).toBe(true);
    expect(result.current.info?.username).toBe("admin");
  });

  it("login error does not update session info", async () => {
    mockInvoke
      .mockResolvedValueOnce(fixtures.noSession)
      .mockRejectedValueOnce(new Error("Identifiant ou mot de passe invalide."));

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => {
      try {
        await result.current.login({ username: "admin", password: "wrong" });
      } catch {
        // expected — login re-throws so the form can react
      }
    });

    expect(result.current.info?.is_authenticated).toBe(false);
    expect(result.current.error).toBeTruthy();
  });

  it("logout clears session", async () => {
    mockInvoke
      .mockResolvedValueOnce(fixtures.authenticatedSession) // initial load
      .mockResolvedValueOnce(undefined); // logout

    const { result } = renderHook(() => useSession());
    await act(async () => {});

    await act(async () => {
      await result.current.logout();
    });
    expect(result.current.info?.is_authenticated).toBe(false);
  });
});

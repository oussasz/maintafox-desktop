/**
 * The forced password-change screen must still appear when the server answers
 * PASSWORD_CHANGE_REQUIRED (session flagged server-side), after the auth screens moved to the
 * shared auth shell. Routing is AuthGuard's: no lock, no toast, the change screen replaces the app.
 */
import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/contexts/PermissionContext", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return {
    ...actual,
    PermissionProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
  };
});

import { AuthGuard } from "@/components/auth/AuthGuard";
import { invoke } from "@/lib/ipc-invoke";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { useSessionStore } from "@/store/session-store";
import { fixtures, mockInvoke } from "@/test/mocks/tauri";

function renderGuarded() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route element={<AuthGuard />}>
          <Route path="/" element={<p>work orders screen</p>} />
        </Route>
        <Route path="/login" element={<p>login screen</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("AuthGuard — PASSWORD_CHANGE_REQUIRED", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    useSessionStore.getState().resetForTests();
    useAuthInterceptorStore.getState().clear();
  });

  it("replaces the app with the forced change screen when the server requires it", async () => {
    useSessionStore.setState({
      info: {
        ...fixtures.authenticatedSession,
        force_password_change: false,
        password_expires_in_days: null,
        pin_configured: false,
      },
      isLoading: false,
      hasBootstrapped: true,
    });
    renderGuarded();
    expect(screen.getByText("work orders screen")).toBeInTheDocument();

    mockInvoke
      .mockRejectedValueOnce({
        code: "PASSWORD_CHANGE_REQUIRED",
        message: "PASSWORD_CHANGE_REQUIRED: le mot de passe doit être changé avant de continuer.",
      })
      .mockResolvedValueOnce(fixtures.authenticatedSession);

    await expect(invoke("list_work_orders")).rejects.toBeTruthy();

    await waitFor(() => {
      expect(screen.queryByText("work orders screen")).not.toBeInTheDocument();
    });
    // Unit tests render translation keys.
    expect(screen.getByLabelText("session.forcePasswordChange.newPassword")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "session.forcePasswordChange.title" }),
    ).toBeInTheDocument();
    expect(useAuthInterceptorStore.getState().isLockOpen).toBe(false);
  });
});

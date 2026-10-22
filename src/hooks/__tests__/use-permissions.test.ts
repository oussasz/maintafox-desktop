import { act, renderHook, waitFor } from "@testing-library/react";
import { type ReactNode, createElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionProvider } from "@/contexts/PermissionContext";
import { usePermissions } from "@/hooks/use-permissions";
import type * as IpcInvoke from "@/lib/ipc-invoke";
import { resetPermissionCacheForTests, writePermissionCache } from "@/lib/permission-cache";
import { P } from "@shared/rbac/permissions.generated";

const mockGetMyPermissions = vi.fn();
const mockSession = vi.fn();

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => mockSession(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => vi.fn()),
}));

const mockConfirmSession = vi.fn(async (_err: unknown) => undefined);
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcInvoke>()),
  confirmSessionAfterSilentFailure: (err: unknown) => mockConfirmSession(err),
}));

const MOCK_PERMISSIONS = [
  {
    name: P.EQ_VIEW,
    description: "View equipment",
    category: "equipment",
    is_dangerous: false,
    requires_step_up: false,
  },
  {
    name: P.EQ_MANAGE,
    description: "Edit equipment",
    category: "equipment",
    is_dangerous: false,
    requires_step_up: false,
  },
  {
    name: P.ADM_USERS,
    description: "Manage users",
    category: "administration",
    is_dangerous: true,
    requires_step_up: true,
  },
];

function authenticatedSession() {
  return {
    info: {
      is_authenticated: true,
      is_locked: false,
      user_id: 1,
      username: "admin",
      display_name: "Admin",
      is_admin: true,
      force_password_change: false,
      expires_at: null,
      last_activity_at: null,
      password_expires_in_days: null,
      pin_configured: false,
      tenant_id: null,
      token_tenant_id: null,
    },
    isLoading: false,
    hasBootstrapped: true,
    error: null,
    errorCode: null,
    login: vi.fn(),
    logout: vi.fn(),
    refresh: vi.fn(),
    unlock: vi.fn(),
    changePassword: vi.fn(),
    ensureBootstrapped: vi.fn(),
    resetForTests: vi.fn(),
  };
}

function wrapper({ children }: { children: ReactNode }) {
  return createElement(PermissionProvider, null, children);
}

describe("usePermissions", () => {
  beforeEach(() => {
    resetPermissionCacheForTests();
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue(MOCK_PERMISSIONS);
    mockConfirmSession.mockClear();
    mockSession.mockReturnValue(authenticatedSession());
  });

  it("loads permissions from backend", async () => {
    const { result } = renderHook(() => usePermissions(), { wrapper });
    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.permissions).toHaveLength(3);
    expect(mockGetMyPermissions).toHaveBeenCalledOnce();
  });

  it("can() returns true for held permission", async () => {
    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can(P.EQ_VIEW)).toBe(true);
    expect(result.current.can(P.EQ_MANAGE)).toBe(true);
    expect(result.current.can(P.ADM_USERS)).toBe(true);
  });

  it("can() returns false for missing permission", async () => {
    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can(P.ADM_ROLES)).toBe(false);
    expect(result.current.can(P.EQ_DELETE)).toBe(false);
  });

  it("refresh() reloads permissions", async () => {
    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    mockGetMyPermissions.mockResolvedValue([
      ...MOCK_PERMISSIONS,
      {
        name: P.OT_VIEW,
        description: "View WO",
        category: "work_order",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    await act(async () => {
      await result.current.refresh();
    });

    await waitFor(() => expect(result.current.permissions).toHaveLength(4));
    expect(result.current.can(P.OT_VIEW)).toBe(true);
  });

  it("does not call getMyPermissions when session is unauthenticated", async () => {
    const base = authenticatedSession();
    if (!base.info) throw new Error("expected authenticated session info");
    mockSession.mockReturnValue({
      ...base,
      info: { ...base.info, is_authenticated: false, user_id: null },
    });

    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(mockGetMyPermissions).not.toHaveBeenCalled();
    expect(result.current.permissions).toHaveLength(0);
  });

  it("keeps prior permissions on transient AUTH_ERROR", async () => {
    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.permissions).toHaveLength(3));

    mockGetMyPermissions.mockRejectedValue({
      code: "AUTH_ERROR",
      message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
    });

    await act(async () => {
      await result.current.refresh();
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.permissions).toHaveLength(3);
    expect(result.current.can(P.EQ_VIEW)).toBe(true);
  });

  it("reloads after an AUTH error once a new session starts (F1)", async () => {
    // Fresh start: no cache; the first load is refused (the session was not usable yet).
    mockGetMyPermissions.mockRejectedValueOnce({
      code: "AUTH_ERROR",
      message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
    });
    mockSession.mockReturnValue({ ...authenticatedSession(), sessionGeneration: 1 });
    const { result, rerender } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.permissions).toHaveLength(0);
    expect(mockConfirmSession).toHaveBeenCalledOnce();

    // Signing in again (or an authentication race) raises the session generation: reload.
    mockSession.mockReturnValue({ ...authenticatedSession(), sessionGeneration: 2 });
    rerender();
    await waitFor(() => expect(result.current.permissions).toHaveLength(3));
    expect(result.current.can(P.ADM_USERS)).toBe(true);
    expect(mockGetMyPermissions).toHaveBeenCalledTimes(2);
  });

  it("restores cached permissions after remount when reload fails", async () => {
    writePermissionCache(1, MOCK_PERMISSIONS);
    mockGetMyPermissions.mockRejectedValue({
      code: "AUTH_ERROR",
      message: "Authentication error: Session expirée ou absente. Veuillez vous reconnecter.",
    });

    const { result } = renderHook(() => usePermissions(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.permissions).toHaveLength(3);
    expect(result.current.can(P.EQ_VIEW)).toBe(true);
  });
});

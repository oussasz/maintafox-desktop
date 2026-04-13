import { renderHook, waitFor, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { usePermissions } from "@/hooks/use-permissions";

// Mock the service layer — not Tauri invoke directly.
// This isolates the hook logic from IPC transport concerns.
const mockGetMyPermissions = vi.fn();

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

const MOCK_PERMISSIONS = [
  {
    name: "eq.view",
    description: "View equipment",
    category: "equipment",
    is_dangerous: false,
    requires_step_up: false,
  },
  {
    name: "eq.manage",
    description: "Edit equipment",
    category: "equipment",
    is_dangerous: false,
    requires_step_up: false,
  },
  {
    name: "adm.users",
    description: "Manage users",
    category: "administration",
    is_dangerous: true,
    requires_step_up: true,
  },
];

describe("usePermissions", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue(MOCK_PERMISSIONS);
  });

  it("loads permissions from backend", async () => {
    const { result } = renderHook(() => usePermissions());
    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.permissions).toHaveLength(3);
    expect(mockGetMyPermissions).toHaveBeenCalledOnce();
  });

  it("can() returns true for held permission", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can("eq.view")).toBe(true);
    expect(result.current.can("eq.manage")).toBe(true);
    expect(result.current.can("adm.users")).toBe(true);
  });

  it("can() returns false for missing permission", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.can("adm.roles")).toBe(false);
    expect(result.current.can("eq.delete")).toBe(false);
  });

  it("refresh() reloads permissions", async () => {
    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    // Update the mock to return a different set
    mockGetMyPermissions.mockResolvedValue([
      ...MOCK_PERMISSIONS,
      {
        name: "ot.view",
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
    expect(result.current.can("ot.view")).toBe(true);
  });

  it("sets empty permissions on backend error", async () => {
    mockGetMyPermissions.mockRejectedValue(new Error("IPC failure"));

    const { result } = renderHook(() => usePermissions());
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.permissions).toHaveLength(0);
    expect(result.current.can("eq.view")).toBe(false);
  });
});

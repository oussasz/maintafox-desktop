/**
 * Role editor: unsaved changes are not lost by switching Administration tab (a change of the
 * `?tab=` query) — the shared guard asks first; "Cancel" stays, "Discard" goes on.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { RoleEditorPanel } from "@/components/admin/RoleEditorPanel";
import type { RoleEditorCatalog, RoleWithPermissions } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: (f: () => unknown) => f(), StepUpDialogElement: null }),
}));

const svc = vi.hoisted(() => ({
  cloneRole: vi.fn(),
  createRole: vi.fn(),
  deleteRole: vi.fn(),
  getRoleEditorCatalog: vi.fn(),
  listRoleTemplates: vi.fn(),
  listRoles: vi.fn(),
  updateRole: vi.fn(),
  validateRolePermissions: vi.fn(),
}));
vi.mock("@/services/rbac-service", () => svc);

// The data router builds a fetch Request per navigation; jsdom's AbortSignal is not the one
// Node's Request accepts, so the signal is dropped here (no loaders run in this route).
const NativeRequest = globalThis.Request;
beforeAll(() => {
  globalThis.Request = class extends NativeRequest {
    constructor(input: RequestInfo | URL, init?: RequestInit) {
      const rest = { ...init };
      delete rest.signal;
      super(input, rest);
    }
  } as typeof Request;
});
afterAll(() => {
  globalThis.Request = NativeRequest;
});

const ROLE: RoleWithPermissions = {
  id: 1,
  name: "Planner",
  description: "Plans the work",
  role_type: "custom",
  status: "active",
  is_system: false,
  permissions: [],
};
const CATALOG: RoleEditorCatalog = { permissions: [], dependencies: [], role_holders: [] };

describe("RoleEditorPanel unsaved changes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listRoles.mockResolvedValue([ROLE]);
    svc.listRoleTemplates.mockResolvedValue([]);
    svc.getRoleEditorCatalog.mockResolvedValue(CATALOG);
  });

  it("asks before switching Administration tab with unsaved changes", async () => {
    const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
      initialEntries: ["/admin?tab=roles"],
    });
    render(<RouterProvider router={router} />);

    fireEvent.click(await screen.findByRole("button", { name: /Planner/ }));
    fireEvent.change(await screen.findByLabelText("roles.fields.description"), {
      target: { value: "Plans and schedules the work" },
    });

    // Another Administration tab is a change of the ?tab= query.
    await act(async () => {
      await router.navigate("/admin?tab=users");
    });
    expect(await screen.findByText("console.roleEditor.unsavedLeaveBody")).toBeInTheDocument();
    expect(router.state.location.search).toBe("?tab=roles");

    fireEvent.click(screen.getByRole("button", { name: "console.roleEditor.discardChanges" }));
    await waitFor(() => expect(router.state.location.search).toBe("?tab=users"));
  });
});

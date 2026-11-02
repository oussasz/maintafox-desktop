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

  it("asks before creating another role with unsaved ticks, and keeps them on Cancel", async () => {
    const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
      initialEntries: ["/admin?tab=roles"],
    });
    render(<RouterProvider router={router} />);

    fireEvent.click(await screen.findByRole("button", { name: /Planner/ }));
    fireEvent.change(await screen.findByLabelText("roles.fields.description"), {
      target: { value: "Plans and schedules the work" },
    });
    // The save bar is there while something is unsaved.
    expect(screen.getByRole("region", { name: "console.roleEditor.saveBar" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /console\.roleEditor\.new/ }));
    expect(await screen.findByText("console.roleEditor.unsavedNewBody")).toBeInTheDocument();
    // The create dialog waits for the answer.
    expect(screen.queryByText("roles.create.title")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "ds.confirm.cancel" }));
    await waitFor(() => expect(screen.queryByText("console.roleEditor.unsavedNewBody")).toBeNull());
    expect(screen.getByLabelText("roles.fields.description")).toHaveValue(
      "Plans and schedules the work",
    );
  });
});

describe("RoleEditorPanel whole-group switch", () => {
  const PERMS: RoleEditorCatalog = {
    permissions: [
      {
        id: 1,
        name: "doc.view",
        description: "View docs",
        category: "doc",
        is_dangerous: false,
        requires_step_up: false,
        is_system: true,
      },
      {
        id: 2,
        name: "doc.manage",
        description: "Edit docs",
        category: "doc",
        is_dangerous: false,
        requires_step_up: false,
        is_system: true,
      },
    ],
    dependencies: [],
    role_holders: [],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    svc.listRoles.mockResolvedValue([ROLE]);
    svc.listRoleTemplates.mockResolvedValue([]);
    svc.getRoleEditorCatalog.mockResolvedValue(PERMS);
    svc.validateRolePermissions.mockResolvedValue({
      missing_hard_deps: [],
      warn_deps: [],
      unknown_permissions: [],
      is_valid: true,
    });
    svc.updateRole.mockResolvedValue(undefined);
  });

  it("ticks every right of the group at once, saved as one change", async () => {
    const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
      initialEntries: ["/admin?tab=roles"],
    });
    render(<RouterProvider router={router} />);

    fireEvent.click(await screen.findByRole("button", { name: /Planner/ }));
    fireEvent.click(
      await screen.findByRole("switch", { name: "console.roleEditor.wholeGroupToggle" }),
    );

    const switches = screen.getAllByRole("switch", { name: "roles.togglePermission" });
    expect(switches.every((s) => s.getAttribute("aria-checked") === "true")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "console.roleEditor.save" }));
    await waitFor(() => expect(svc.updateRole).toHaveBeenCalled());
    const input = svc.updateRole.mock.calls[0]?.[0] as { add_permissions: string[] };
    expect([...input.add_permissions].sort()).toEqual(["doc.manage", "doc.view"]);
  });
});

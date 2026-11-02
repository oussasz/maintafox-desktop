/**
 * Role editor: unsaved changes are not lost by switching Administration tab (a change of the
 * `?tab=` query) — the shared guard asks first; "Cancel" stays, "Discard" goes on.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { RoleEditorPanel } from "@/components/admin/RoleEditorPanel";
import type { RoleEditorCatalog, RoleWithPermissions } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

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
        name: P.DOC_VIEW,
        description: "View docs",
        category: "doc",
        is_dangerous: false,
        requires_step_up: false,
        is_system: true,
      },
      {
        id: 2,
        name: P.DOC_MANAGE,
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
    expect([...input.add_permissions].sort()).toEqual([P.DOC_MANAGE, P.DOC_VIEW]);
  });
});

describe("RoleEditorPanel prerequisites (L5-06)", () => {
  const perm = (id: number, name: string) => ({
    id,
    name,
    description: name,
    category: name.split(".")[0] ?? name,
    is_dangerous: false,
    requires_step_up: false,
    is_system: true,
  });
  const CAT: RoleEditorCatalog = {
    permissions: [
      perm(1, P.DOC_VIEW),
      perm(2, P.DOC_MANAGE),
      perm(3, P.DI_VIEW),
      perm(4, P.DI_REVIEW),
      perm(5, P.DI_APPROVE),
    ],
    dependencies: [
      {
        id: 1,
        permission_name: P.DOC_MANAGE,
        required_permission_name: P.DOC_VIEW,
        dependency_type: "hard",
      },
      {
        id: 2,
        permission_name: P.DI_APPROVE,
        required_permission_name: P.DI_REVIEW,
        dependency_type: "hard",
      },
    ],
    role_holders: [],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    svc.listRoles.mockResolvedValue([ROLE]);
    svc.listRoleTemplates.mockResolvedValue([]);
    svc.getRoleEditorCatalog.mockResolvedValue(CAT);
    svc.validateRolePermissions.mockResolvedValue({
      missing_hard_deps: [],
      warn_deps: [],
      unknown_permissions: [],
      is_valid: true,
    });
  });

  async function openRole() {
    const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
      initialEntries: ["/admin?tab=roles"],
    });
    render(<RouterProvider router={router} />);
    fireEvent.click(await screen.findByRole("button", { name: /Planner/ }));
    return await screen.findAllByRole("switch", { name: "roles.togglePermission" });
  }

  it("ticks the view right a right needs, says why, and never ticks an approval prerequisite", async () => {
    await openRole();
    const row = (name: string) =>
      screen.getAllByRole("listitem").find((li) => li.querySelector(`[title="${name}"]`));
    const toggleOf = (name: string) => row(name)?.querySelector('[role="switch"]') as HTMLElement;

    fireEvent.click(toggleOf(P.DOC_MANAGE));
    expect(toggleOf(P.DOC_VIEW).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText("console.roleEditor.autoAdded")).toBeInTheDocument();

    fireEvent.click(toggleOf(P.DI_APPROVE));
    expect(toggleOf(P.DI_REVIEW).getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText("console.roleEditor.notAddedTitle")).toBeInTheDocument();
  });

  it("warns when a prerequisite is unticked, and leaves the right that needs it ticked", async () => {
    await openRole();
    const row = (name: string) =>
      screen.getAllByRole("listitem").find((li) => li.querySelector(`[title="${name}"]`));
    const toggleOf = (name: string) => row(name)?.querySelector('[role="switch"]') as HTMLElement;

    fireEvent.click(toggleOf(P.DOC_MANAGE));
    fireEvent.click(toggleOf(P.DOC_VIEW));
    expect(toggleOf(P.DOC_VIEW).getAttribute("aria-checked")).toBe("false");
    expect(toggleOf(P.DOC_MANAGE).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByText("console.roleEditor.dependentsTitle")).toBeInTheDocument();
  });
});

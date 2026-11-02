/**
 * Role editor, shipped roles (L5-03): shown by their translated name, the former duplicate of a
 * shipped role left out of the list, a role made by the plant under its own name.
 */
import { render, screen } from "@testing-library/react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { RoleEditorPanel } from "@/components/admin/RoleEditorPanel";
import type { RoleEditorCatalog, RoleWithPermissions } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
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

function role(id: number, name: string, is_system: boolean): RoleWithPermissions {
  return {
    id,
    name,
    description: "stored text",
    role_type: is_system ? "system" : "custom",
    status: "active",
    is_system,
    permissions: [],
  };
}
const CATALOG: RoleEditorCatalog = { permissions: [], dependencies: [], role_holders: [] };

describe("RoleEditorPanel shipped roles", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listRoles.mockResolvedValue([
      role(1, "Administrator", true),
      role(2, "Superadmin", true),
      role(3, "Chef d'équipe", false),
    ]);
    svc.listRoleTemplates.mockResolvedValue([]);
    svc.getRoleEditorCatalog.mockResolvedValue(CATALOG);
  });

  it("lists shipped roles by their translated name and leaves the former duplicate out", async () => {
    const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
      initialEntries: ["/admin?tab=roles"],
    });
    render(<RouterProvider router={router} />);

    expect(
      await screen.findByRole("button", { name: /roles\.shipped\.administrator\.name/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Chef d'équipe/ })).toBeInTheDocument();
    expect(screen.queryByText(/roles\.shipped\.formerRole/)).toBeNull();
    expect(screen.queryByText("Superadmin")).toBeNull();
    expect(screen.queryByText("Administrator")).toBeNull();
  });
});

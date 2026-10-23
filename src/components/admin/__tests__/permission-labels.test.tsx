/**
 * Permission names on the administration screens (F12, AU-47): the role editor and the account
 * Permissions tab show the French label, never the code; goods-receipt validation sits with the
 * stores permissions; the permissions unused in this version are not offered, and a role that
 * holds some keeps them on save (a read-only line says how many).
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionCatalogPanel } from "@/components/admin/PermissionCatalogPanel";
import { RoleEditorPanel } from "@/components/admin/RoleEditorPanel";
import { UserDetailDialog } from "@/components/admin/UserDetailDialog";
import adminFr from "@/i18n/locale-data/fr/admin.json";
import type {
  PermissionWithSystem,
  RoleEditorCatalog,
  RoleWithPermissions,
  UserDetail,
} from "@shared/ipc-types";
import { HIDDEN_FROM_PLANT_SCREENS, P } from "@shared/rbac/permissions.generated";

/** `t` over the real French admin texts: plural forms, {{interpolation}}, defaultValue. */
const i18nMock = vi.hoisted(() => {
  const state: { resources: Record<string, unknown> } = { resources: {} };
  const lookup = (key: string): unknown =>
    key
      .split(".")
      .reduce<unknown>(
        (node, part) =>
          node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined,
        state.resources,
      );
  const t = (key: string, opts?: unknown): string => {
    const o = (typeof opts === "object" && opts !== null ? opts : {}) as Record<string, unknown>;
    let found: unknown;
    if (typeof o["count"] === "number") {
      found = lookup(`${key}_${o["count"] === 1 ? "one" : "other"}`);
    }
    found ??= lookup(key);
    const fallback = typeof opts === "string" ? opts : (o["defaultValue"] as string | undefined);
    const text = typeof found === "string" ? found : (fallback ?? key);
    return text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(o[name] ?? ""));
  };
  return { state, value: { t, i18n: { language: "fr" } } };
});
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock.value }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: (f: () => unknown) => f(), StepUpDialogElement: null }),
}));
vi.mock("@/components/admin/AdminAuditTimeline", () => ({
  AdminAuditTimeline: () => <div data-testid="history" />,
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
  getUser: vi.fn(),
  listActiveSessions: vi.fn(),
  listPermissions: vi.fn(),
  getPermissionDependencies: vi.fn(),
  createCustomPermission: vi.fn(),
}));
vi.mock("@/services/rbac-service", () => svc);

// The data router builds a fetch Request per navigation; jsdom's AbortSignal is not the one
// Node's Request accepts, so the signal is dropped here (no loaders run in this route).
const NativeRequest = globalThis.Request;
beforeAll(() => {
  i18nMock.state.resources = adminFr as Record<string, unknown>;
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

const HIDDEN = [...HIDDEN_FROM_PLANT_SCREENS][0] as string;

function perm(id: number, name: string, description: string): PermissionWithSystem {
  return {
    id,
    name,
    description,
    category: name.split(".")[0] ?? name,
    is_dangerous: false,
    requires_step_up: false,
    is_system: true,
  };
}

/** What the backend offers: the hidden permissions are already left out. */
const CATALOG: RoleEditorCatalog = {
  permissions: [
    perm(1, P.OT_VIEW, "View work orders"),
    perm(2, P.OT_CLOSE, "Close work orders"),
    perm(3, P.INV_VIEW, "View inventory"),
    perm(4, P.ERP_RECONCILE, "Reconcile goods receipts"),
  ],
  dependencies: [],
  role_holders: [],
};

const ROLE: RoleWithPermissions = {
  id: 1,
  name: "Magasinier",
  description: null,
  role_type: "custom",
  status: "active",
  is_system: false,
  // Holds one permission unused in this version and one vendor console right.
  permissions: [P.OT_VIEW, HIDDEN, P.CONSOLE_VIEW],
};

function renderEditor() {
  const router = createMemoryRouter([{ path: "/admin", element: <RoleEditorPanel /> }], {
    initialEntries: ["/admin?tab=roles"],
  });
  render(<RouterProvider router={router} />);
}

describe("role editor permission names", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listRoles.mockResolvedValue([ROLE]);
    svc.listRoleTemplates.mockResolvedValue([]);
    svc.getRoleEditorCatalog.mockResolvedValue(CATALOG);
    svc.validateRolePermissions.mockResolvedValue({
      missing_hard_deps: [],
      warn_deps: [],
      unknown_permissions: [],
      is_valid: true,
    });
    svc.updateRole.mockResolvedValue(undefined);
  });

  it("shows labels, not codes, and puts goods-receipt validation with the stores rights", async () => {
    renderEditor();
    fireEvent.click(await screen.findByRole("button", { name: /Magasinier/ }));

    expect(await screen.findByText("Clôturer les ordres de travail")).toBeInTheDocument();
    expect(screen.getByText("Voir les ordres de travail")).toBeInTheDocument();
    for (const code of [P.OT_VIEW, P.OT_CLOSE, P.INV_VIEW, P.ERP_RECONCILE]) {
      expect(screen.queryByText(code)).not.toBeInTheDocument();
    }

    const reconcile = screen.getByText("Valider les réceptions de marchandises (rapprochement)");
    const group = reconcile.closest("details") as HTMLElement;
    expect(within(group).getByText(adminFr.permissions.domainLabels.inv)).toBeInTheDocument();
    expect(within(group).getByText("Voir le stock")).toBeInTheDocument();
  });

  it("says how many unused permissions the role keeps, and a save leaves them alone", async () => {
    renderEditor();
    fireEvent.click(await screen.findByRole("button", { name: /Magasinier/ }));

    expect(
      await screen.findByText("2 droits non utilisés dans cette version conservés"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("switch", {
        name: "Activer ou désactiver Clôturer les ordres de travail",
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Enregistrer (+1 / −0)" }));

    await waitFor(() =>
      expect(svc.updateRole).toHaveBeenCalledWith({
        role_id: 1,
        add_permissions: [P.OT_CLOSE],
        remove_permissions: [],
      }),
    );
  });
});

describe("permission catalogue", () => {
  it("lists goods-receipt validation under stores by label, and no empty IoT/ERP domain", async () => {
    svc.listPermissions.mockResolvedValue(CATALOG.permissions);
    render(<PermissionCatalogPanel />);

    const stores = await screen.findByRole("button", {
      name: new RegExp(adminFr.permissions.domainLabels.inv),
    });
    expect(screen.queryByText(adminFr.permissions.domainLabels.iot)).not.toBeInTheDocument();
    expect(screen.queryByText(adminFr.permissions.domainLabels.erp)).not.toBeInTheDocument();
    fireEvent.click(stores);

    expect(
      await screen.findByRole("button", {
        name: /Valider les réceptions de marchandises \(rapprochement\)/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText(adminFr.permissions.items.inv_view.description)).toBeInTheDocument();
  });
});

describe("account Permissions tab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listActiveSessions.mockResolvedValue([]);
  });

  it("lists the effective permissions by label, without the unused ones", async () => {
    const detail: UserDetail = {
      user: {
        id: 7,
        username: "j.doe",
        display_name: "J. Doe",
        personnel_id: null,
        email: null,
        phone: null,
        identity_mode: "local",
        is_active: true,
        force_password_change: false,
        last_seen_at: null,
        last_login_at: null,
        locked_until: null,
        is_locked: false,
        roles: [],
      },
      scope_assignments: [],
      effective_permissions: [P.OT_CLOSE, P.ERP_RECONCILE, HIDDEN],
    };
    svc.getUser.mockResolvedValue(detail);
    render(
      <UserDetailDialog
        userId={7}
        open
        onClose={vi.fn()}
        onChanged={vi.fn()}
        onEdit={vi.fn()}
        onTemporaryPassword={vi.fn()}
        withStepUp={(action) => action()}
        roles={[]}
        nodes={[]}
        currentUserId={1}
      />,
    );
    fireEvent.mouseDown(await screen.findByRole("tab", { name: "Permissions" }), { button: 0 });

    expect(await screen.findByText("Clôturer les ordres de travail")).toBeInTheDocument();
    expect(screen.queryByText(P.OT_CLOSE)).not.toBeInTheDocument();
    const reconcile = screen.getByText("Valider les réceptions de marchandises (rapprochement)");
    expect(reconcile.parentElement?.previousElementSibling).toHaveTextContent(
      adminFr.permissions.domainLabels.inv,
    );
    expect(screen.queryByText(HIDDEN)).not.toBeInTheDocument();
    expect(screen.getByText(/^2 permissions effectives/)).toBeInTheDocument();
  });
});

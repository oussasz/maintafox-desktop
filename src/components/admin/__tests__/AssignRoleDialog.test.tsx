import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssignRoleDialog } from "@/components/admin/AssignRoleDialog";
import { assignRoleScope } from "@/services/rbac-service";
import type {
  AssignableRoleSummary,
  AssignableScopeNode,
  RoleAssignmentSummary,
  UserWithRoles,
} from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && typeof opts === "object" ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "fr" },
  }),
}));

vi.mock("@/services/rbac-service", () => ({ assignRoleScope: vi.fn() }));

// The two selects (role, scope) as plain option buttons.
vi.mock("@/components/ui/select", async () => {
  const React = await import("react");
  const Ctx = React.createContext<(v: string) => void>(() => {});
  return {
    Select: ({
      onValueChange,
      children,
    }: {
      onValueChange: (v: string) => void;
      children: React.ReactNode;
    }) => (
      <Ctx.Provider value={onValueChange}>
        <div>{children}</div>
      </Ctx.Provider>
    ),
    SelectTrigger: () => null,
    SelectValue: () => null,
    SelectContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    SelectItem: ({ value, children }: { value: string; children: React.ReactNode }) => {
      const set = React.useContext(Ctx);
      return (
        <button type="button" role="option" aria-selected={false} onClick={() => set(value)}>
          {children}
        </button>
      );
    },
  };
});

const ROLES: AssignableRoleSummary[] = [
  {
    id: 41,
    name: "Tech Sétif",
    description: null,
    role_type: "custom",
    status: "active",
    is_system: false,
  },
];

const NODES: AssignableScopeNode[] = [
  {
    id: 1,
    parent_id: null,
    code: "TENANT-ROOT-7F3A",
    name: "Sonelgaz Est",
    type_label: "Entreprise",
    depth: 0,
  },
  { id: 5, parent_id: 1, code: "SETIF", name: "Usine de Sétif", type_label: "Site", depth: 1 },
];

function role(
  id: number,
  name: string,
  scope_type: string,
  scope_reference: string | null,
  extra: Partial<RoleAssignmentSummary> = {},
): RoleAssignmentSummary {
  return {
    assignment_id: id,
    role_id: id,
    role_name: name,
    scope_type,
    scope_reference,
    valid_from: null,
    valid_to: null,
    is_emergency: false,
    emergency_expires_at: null,
    scope_label: null,
    ...extra,
  };
}

function user(roles: RoleAssignmentSummary[]): UserWithRoles {
  return {
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
    roles,
  };
}

function renderDialog(roles: RoleAssignmentSummary[]) {
  const onAssigned = vi.fn();
  render(
    <AssignRoleDialog
      user={user(roles)}
      open
      onClose={vi.fn()}
      roles={ROLES}
      nodes={NODES}
      onAssigned={onAssigned}
      withStepUp={(action) => action()}
    />,
  );
  return { onAssigned };
}

function choose(roleName: string, scopeText: string) {
  fireEvent.click(screen.getByRole("option", { name: new RegExp(roleName) }));
  fireEvent.click(screen.getByRole("option", { name: new RegExp(scopeText) }));
}

const PLANT = "^console\\.scope\\.plant$";

beforeEach(() => {
  vi.mocked(assignRoleScope).mockReset();
  vi.mocked(assignRoleScope).mockResolvedValue({ id: 99 });
});

describe("AssignRoleDialog warnings wait for a real choice", () => {
  it("says nothing before a role and a scope are chosen", () => {
    renderDialog([role(1, "Plant lead", "tenant", null)]);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByTestId("assign-plant-wide")).toBeNull();
    expect(screen.getByRole("button", { name: "console.assign.confirm" })).toBeDisabled();
  });

  it("names a plant-wide role replaced by a plant-wide choice", () => {
    renderDialog([
      role(1, "Plant lead", "tenant", "plant-01"),
      role(2, "Site tech", "org_node", "5"),
    ]);
    choose("Tech Sétif", PLANT);
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("console.assign.replaces");
    expect(status.textContent).toContain("Plant lead");
    expect(status.textContent).not.toContain("Site tech");
  });

  it("names every plant-wide role, whatever reference each was recorded with", () => {
    renderDialog([
      role(1, "Plant lead", "tenant", "plant-01"),
      role(2, "Reader", "tenant", null),
      role(3, "Site tech", "org_node", "5"),
    ]);
    choose("Tech Sétif", PLANT);
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("console.assign.replacesMany");
    expect(status.textContent).toContain("Plant lead, Reader");
  });
});

describe("AssignRoleDialog: a site role replaces the plant-wide one (L5-07)", () => {
  it("offers to end the plant-wide role, ticked by default, and sends it with the grant", async () => {
    const { onAssigned } = renderDialog([role(11, "Plant lead", "tenant", null)]);
    choose("Tech Sétif", "Usine de Sétif");
    const box = screen.getByTestId("assign-plant-wide");
    const tick = screen.getByRole("checkbox");
    expect(tick).toBeChecked();
    expect(box.textContent).toContain("console.assign.plantWide.replace");
    expect(box.textContent).toContain("Plant lead");
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "console.assign.confirm" }));
    await waitFor(() => expect(onAssigned).toHaveBeenCalled());
    expect(assignRoleScope).toHaveBeenCalledWith(
      expect.objectContaining({
        role_id: 41,
        scope_type: "org_node",
        scope_reference: "5",
        end_plant_wide_assignment_ids: [11],
      }),
    );
  });

  it("warns that a kept plant-wide role still opens every site, and keeps it", async () => {
    const { onAssigned } = renderDialog([role(11, "Plant lead", "tenant", null)]);
    choose("Tech Sétif", "Usine de Sétif");
    fireEvent.click(screen.getByRole("checkbox"));
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("console.assign.plantWide.kept");
    expect(alert.textContent).toContain("Plant lead");

    fireEvent.click(screen.getByRole("button", { name: "console.assign.confirm" }));
    await waitFor(() => expect(onAssigned).toHaveBeenCalled());
    expect(assignRoleScope).toHaveBeenCalledWith(
      expect.objectContaining({ end_plant_wide_assignment_ids: [] }),
    );
  });

  it("lets each of several plant-wide roles be kept on its own", async () => {
    renderDialog([
      role(11, "Plant lead", "tenant", null),
      role(12, "Reader", "org_node", "1"),
      role(13, "Old role", "tenant", null, { valid_to: "2000-01-01T23:59:59Z" }),
      role(14, "Urgence", "tenant", null, { is_emergency: true }),
      role(15, "Other site", "org_node", "9"),
    ]);
    choose("Tech Sétif", "Usine de Sétif");
    const ticks = screen.getAllByRole("checkbox");
    // The top node counts as the whole plant; expired, emergency and other-site roles do not.
    expect(ticks).toHaveLength(2);
    fireEvent.click(ticks[1] as HTMLElement);
    fireEvent.click(screen.getByRole("button", { name: "console.assign.confirm" }));
    await waitFor(() =>
      expect(assignRoleScope).toHaveBeenCalledWith(
        expect.objectContaining({ end_plant_wide_assignment_ids: [11] }),
      ),
    );
  });

  it("offers nothing to replace for a plant-wide choice", () => {
    renderDialog([role(11, "Plant lead", "tenant", null)]);
    choose("Tech Sétif", PLANT);
    expect(screen.queryByTestId("assign-plant-wide")).toBeNull();
  });
});

describe("AssignRoleDialog scope names", () => {
  it("lists the whole plant once, by name, never the internal code of the top node", () => {
    renderDialog([]);
    const options = screen.getAllByRole("option").map((o) => o.textContent ?? "");
    expect(options).toContain("console.scope.plant");
    expect(options.join(" ")).not.toContain("TENANT-ROOT");
    expect(options.join(" ")).not.toContain("Sonelgaz Est");
    expect(options.some((o) => o.includes("Usine de Sétif") && o.includes("Site · SETIF"))).toBe(
      true,
    );
  });
});

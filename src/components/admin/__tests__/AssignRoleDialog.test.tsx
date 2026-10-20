import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AssignRoleDialog } from "@/components/admin/AssignRoleDialog";
import type { RoleAssignmentSummary, UserWithRoles } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && typeof opts === "object" ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "fr" },
  }),
}));

vi.mock("@/services/rbac-service", () => ({ assignRoleScope: vi.fn() }));

function role(
  name: string,
  scope_type: string,
  scope_reference: string | null,
): RoleAssignmentSummary {
  return {
    assignment_id: name.length,
    role_id: name.length,
    role_name: name,
    scope_type,
    scope_reference,
    valid_from: null,
    valid_to: null,
    is_emergency: false,
    emergency_expires_at: null,
    scope_label: null,
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
  render(
    <AssignRoleDialog
      user={user(roles)}
      open
      onClose={vi.fn()}
      roles={[]}
      nodes={[]}
      onAssigned={vi.fn()}
      withStepUp={(action) => action()}
    />,
  );
}

describe("AssignRoleDialog replacement warning (plant-wide, the default target)", () => {
  it("names a plant-wide role recorded with the plant's reference", () => {
    renderDialog([role("Administrator", "tenant", "plant-01"), role("Site tech", "org_node", "5")]);
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("console.assign.replaces");
    expect(status.textContent).toContain("Administrator");
    expect(status.textContent).not.toContain("Site tech");
  });

  it("names every plant-wide role, whatever reference each was recorded with", () => {
    renderDialog([
      role("Administrator", "tenant", "plant-01"),
      role("Reader", "tenant", null),
      role("Site tech", "org_node", "5"),
    ]);
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("console.assign.replacesMany");
    expect(status.textContent).toContain("Administrator, Reader");
    expect(status.textContent).not.toContain("Site tech");
  });

  it("warns of nothing when the user holds only roles limited to a node", () => {
    renderDialog([role("Site tech", "org_node", "5")]);
    expect(screen.queryByRole("status")).toBeNull();
  });
});

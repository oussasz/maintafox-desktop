import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UserDetailDialog } from "@/components/admin/UserDetailDialog";
import type { UserDetail } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: unknown) => (typeof opts === "string" ? opts : key),
    i18n: { language: "fr" },
  }),
}));

vi.mock("@/components/admin/AdminAuditTimeline", () => ({
  AdminAuditTimeline: () => <div data-testid="history" />,
}));

const rbac = vi.hoisted(() => ({
  getUser: vi.fn(),
  listActiveSessions: vi.fn(),
  deactivateUser: vi.fn(),
  reactivateUser: vi.fn(),
  unlockUserAccount: vi.fn(),
  adminSetUserPassword: vi.fn(),
  revokeRoleScope: vi.fn(),
  revokeEmergencyElevation: vi.fn(),
  revokeSession: vi.fn(),
  assignRoleScope: vi.fn(),
}));
vi.mock("@/services/rbac-service", () => rbac);

function detail(overrides: Partial<UserDetail["user"]> = {}): UserDetail {
  return {
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
      ...overrides,
    },
    scope_assignments: [],
    effective_permissions: [P.OT_VIEW],
  };
}

function renderDialog(currentUserId: number | null = 1) {
  const onChanged = vi.fn();
  render(
    <UserDetailDialog
      userId={7}
      open
      onClose={vi.fn()}
      onChanged={onChanged}
      onEdit={vi.fn()}
      onTemporaryPassword={vi.fn()}
      withStepUp={(action) => action()}
      roles={[]}
      nodes={[]}
      currentUserId={currentUserId}
    />,
  );
  return { onChanged };
}

beforeEach(() => {
  for (const fn of Object.values(rbac)) fn.mockReset();
  rbac.listActiveSessions.mockResolvedValue([]);
});

describe("UserDetailDialog", () => {
  it("disables an account only with a reason, then refreshes the list", async () => {
    rbac.getUser.mockResolvedValue(detail());
    rbac.deactivateUser.mockResolvedValue(undefined);
    const { onChanged } = renderDialog();

    fireEvent.click(await screen.findByRole("button", { name: "console.user.disable" }));
    const confirm = await screen.findAllByRole("button", { name: "console.user.disable" });
    const confirmButton = confirm[confirm.length - 1] as HTMLElement;
    expect(confirmButton).toBeDisabled();

    fireEvent.change(screen.getByLabelText("console.reason.required"), {
      target: { value: "Left the company" },
    });
    expect(confirmButton).toBeEnabled();
    fireEvent.click(confirmButton);

    await waitFor(() =>
      expect(rbac.deactivateUser).toHaveBeenCalledWith({ user_id: 7, reason: "Left the company" }),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("offers re-enabling for a disabled account and unlocking for a locked one", async () => {
    rbac.getUser.mockResolvedValue(detail({ is_active: false, is_locked: true }));
    renderDialog();
    expect(await screen.findByRole("button", { name: "console.user.enable" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "console.user.unlock" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "console.user.disable" })).not.toBeInTheDocument();
  });

  it("does not let an administrator disable their own account", async () => {
    rbac.getUser.mockResolvedValue(detail());
    renderDialog(7);
    expect(await screen.findByRole("button", { name: "console.user.disable" })).toBeDisabled();
  });

  it("offers a password reset for another account (AU-36)", async () => {
    rbac.getUser.mockResolvedValue(detail());
    renderDialog(1);
    expect(await screen.findByRole("button", { name: "console.user.reset" })).toBeInTheDocument();
  });

  it("does not offer a password reset on one's own account (AU-36)", async () => {
    rbac.getUser.mockResolvedValue(detail());
    renderDialog(7);
    await screen.findByRole("button", { name: "console.user.disable" });
    expect(screen.queryByRole("button", { name: "console.user.reset" })).not.toBeInTheDocument();
  });

  it("does not offer to give a role to one's own account (AU-45)", async () => {
    rbac.getUser.mockResolvedValue(detail());
    renderDialog(7);
    fireEvent.mouseDown(await screen.findByRole("tab", { name: "console.user.tabs.roles" }), {
      button: 0,
    });
    const assign = await screen.findByRole("button", { name: "console.roles.assign" });
    expect(assign).toBeDisabled();
    expect(assign).toHaveAttribute("title", "permission.cannotGrantOwnRole");
  });

  it("offers giving a role to another account", async () => {
    rbac.getUser.mockResolvedValue(detail());
    renderDialog(1);
    fireEvent.mouseDown(await screen.findByRole("tab", { name: "console.user.tabs.roles" }), {
      button: 0,
    });
    expect(await screen.findByRole("button", { name: "console.roles.assign" })).toBeEnabled();
  });
});

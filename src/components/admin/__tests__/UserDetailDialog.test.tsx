import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UserDetailDialog } from "@/components/admin/UserDetailDialog";
import type { UserDetail } from "@shared/ipc-types";

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
    effective_permissions: ["ot.view"],
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
});

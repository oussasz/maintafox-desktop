import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { PermissionProvider } from "@/contexts/PermissionContext";
import { P } from "@shared/rbac/permissions.generated";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

// PermissionContext only loads permissions for an authenticated session.
vi.mock("@/services/auth-service", () => ({
  getSessionInfo: vi.fn().mockResolvedValue({
    is_authenticated: true,
    is_locked: false,
    user_id: 1,
    username: "admin",
    display_name: "Admin",
    is_admin: true,
    force_password_change: false,
    expires_at: null,
    last_activity_at: null,
    password_expires_in_days: null,
    pin_configured: false,
    tenant_id: "t1",
    token_tenant_id: "t1",
  }),
}));

const archiveMocks = vi.hoisted(() => ({
  listRetentionPolicies: vi.fn(() =>
    Promise.resolve([
      {
        id: 9,
        module_code: "archive",
        archive_class: "WO_CLOSED",
        retention_years: 1,
        purge_mode: "manual_approval",
        allow_restore: true,
        allow_purge: false,
        requires_legal_hold_check: true,
      },
    ]),
  ),
  updateRetentionPolicy: vi.fn((_payload?: unknown) => Promise.resolve()),
}));

vi.mock("@/services/archive-service", () => ({
  listRetentionPolicies: () => archiveMocks.listRetentionPolicies(),
  updateRetentionPolicy: (payload: unknown) => archiveMocks.updateRetentionPolicy(payload),
}));

vi.mock("@/services/activity-service", () => ({
  listActivityEvents: vi.fn(() => Promise.resolve([])),
}));

function renderPanel() {
  return render(
    <PermissionProvider>
      <RetentionPolicyPanel />
    </PermissionProvider>,
  );
}

function permission(name: string) {
  return {
    name,
    description: "",
    category: "admin",
    is_dangerous: false,
    requires_step_up: false,
  };
}

describe("RetentionPolicyPanel staged retention edits", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue([permission(P.ADM_SETTINGS)]);
    archiveMocks.listRetentionPolicies.mockClear();
    archiveMocks.updateRetentionPolicy.mockClear();
  });

  it("saves nothing until 'Enregistrer', then sends only the changed fields", async () => {
    renderPanel();
    const retentionInput = (await screen.findByRole("spinbutton")) as HTMLInputElement;
    fireEvent.change(retentionInput, { target: { value: "15" } });
    fireEvent.blur(retentionInput);
    expect(archiveMocks.updateRetentionPolicy).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "retention.save" }));
    await waitFor(() => {
      expect(archiveMocks.updateRetentionPolicy).toHaveBeenCalledTimes(1);
      expect(archiveMocks.updateRetentionPolicy).toHaveBeenCalledWith({
        policy_id: 9,
        retention_years: 15,
      });
    });
  });

  it("asks before a change that loses data (shorter retention, no restore) (L7-20)", async () => {
    archiveMocks.listRetentionPolicies.mockResolvedValueOnce([
      {
        id: 9,
        module_code: "archive",
        archive_class: "WO_CLOSED",
        retention_years: 5,
        purge_mode: "manual_approval",
        allow_restore: true,
        allow_purge: false,
        requires_legal_hold_check: true,
      },
    ]);
    renderPanel();
    const retentionInput = (await screen.findByRole("spinbutton")) as HTMLInputElement;
    fireEvent.change(retentionInput, { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "retention.save" }));

    expect(await screen.findByText("retention.warn.title")).toBeInTheDocument();
    expect(screen.getByText("retention.warn.shorter")).toBeInTheDocument();
    expect(archiveMocks.updateRetentionPolicy).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "retention.warn.confirm" }));
    await waitFor(() => {
      expect(archiveMocks.updateRetentionPolicy).toHaveBeenCalledWith({
        policy_id: 9,
        retention_years: 2,
      });
    });
  });

  it("keeps the edit of a rule the save refused", async () => {
    archiveMocks.updateRetentionPolicy.mockRejectedValueOnce(new Error("refused"));
    renderPanel();
    const retentionInput = (await screen.findByRole("spinbutton")) as HTMLInputElement;
    fireEvent.change(retentionInput, { target: { value: "15" } });
    fireEvent.click(screen.getByRole("button", { name: "retention.save" }));
    await waitFor(() => {
      expect(archiveMocks.listRetentionPolicies).toHaveBeenCalledTimes(2);
    });
    expect(((await screen.findByRole("spinbutton")) as HTMLInputElement).value).toBe("15");
  });

  it("refuses to save years that are not a whole number", async () => {
    renderPanel();
    const retentionInput = (await screen.findByRole("spinbutton")) as HTMLInputElement;
    fireEvent.change(retentionInput, { target: { value: "-1" } });
    expect(screen.getByText("retention.yearsInvalid")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "retention.save" })).toBeDisabled();
  });
});

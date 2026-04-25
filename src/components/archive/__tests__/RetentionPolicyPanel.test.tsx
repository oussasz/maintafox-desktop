import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { PermissionProvider } from "@/contexts/PermissionContext";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
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
    mockGetMyPermissions.mockResolvedValue([permission("adm.settings")]);
    archiveMocks.listRetentionPolicies.mockClear();
    archiveMocks.updateRetentionPolicy.mockClear();
  });

  it("commits retention_years only on blur (not per key stroke)", async () => {
    renderPanel();

    await waitFor(() => {
      expect(archiveMocks.listRetentionPolicies).toHaveBeenCalled();
    });

    const retentionInput = screen.getByRole("spinbutton") as HTMLInputElement;
    fireEvent.change(retentionInput, { target: { value: "15" } });
    expect(archiveMocks.updateRetentionPolicy).not.toHaveBeenCalled();

    fireEvent.blur(retentionInput);
    await waitFor(() => {
      expect(archiveMocks.updateRetentionPolicy).toHaveBeenCalledTimes(1);
      expect(archiveMocks.updateRetentionPolicy).toHaveBeenCalledWith({
        policy_id: 9,
        retention_years: 15,
      });
    });
  });
});

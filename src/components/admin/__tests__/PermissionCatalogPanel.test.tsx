/**
 * Permission catalog: a failed load is shown with a retry (never an empty catalog), and a
 * failed creation says why.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionCatalogPanel } from "@/components/admin/PermissionCatalogPanel";
import type { PermissionWithSystem } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, isLoading: false }),
}));

const toast = vi.hoisted(() => vi.fn());
vi.mock("@/hooks/use-toast", () => ({
  useToast: () => ({ toast, toasts: [], dismiss: vi.fn() }),
}));

const svc = vi.hoisted(() => ({
  listPermissions: vi.fn(),
  createCustomPermission: vi.fn(),
  getPermissionDependencies: vi.fn(),
}));
vi.mock("@/services/rbac-service", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  listPermissions: svc.listPermissions,
  createCustomPermission: svc.createCustomPermission,
  getPermissionDependencies: svc.getPermissionDependencies,
}));

const viewPermission: PermissionWithSystem = {
  id: 1,
  name: P.EQ_VIEW,
  description: "View equipment",
  category: "equipment",
  is_dangerous: false,
  requires_step_up: false,
  is_system: true,
};

describe("PermissionCatalogPanel", () => {
  beforeEach(() => {
    svc.listPermissions.mockReset();
    svc.createCustomPermission.mockReset();
    toast.mockReset();
  });

  it("shows a failed load with a retry instead of an empty catalog", async () => {
    svc.listPermissions
      .mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "database is locked" })
      .mockResolvedValueOnce([viewPermission]);

    render(<PermissionCatalogPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("permissions.errors.loadFailed");
    expect(screen.queryByText("permissions.empty")).not.toBeInTheDocument();
    expect(screen.getByTitle("permissions.export")).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(svc.listPermissions).toHaveBeenCalledTimes(2);
  });

  it("says why a custom permission could not be created", async () => {
    svc.listPermissions.mockResolvedValue([viewPermission]);
    svc.createCustomPermission.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: "Validation failed: cst.duplicate already exists",
    });

    render(<PermissionCatalogPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "permissions.addCustom" }));
    fireEvent.change(screen.getByPlaceholderText("cst.my_permission"), {
      target: { value: "cst.duplicate" },
    });
    fireEvent.click(screen.getByRole("button", { name: "permissions.create.confirm" }));

    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({
          title: "permissions.errors.createFailed",
          description: expect.stringContaining("cst.duplicate already exists"),
          variant: "destructive",
        }),
      ),
    );
  });
});

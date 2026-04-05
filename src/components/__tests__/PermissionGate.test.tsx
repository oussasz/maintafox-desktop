import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { PermissionGate } from "@/components/PermissionGate";

const mockGetMyPermissions = vi.fn();

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

describe("PermissionGate", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
  });

  it("renders children when user has the permission", async () => {
    mockGetMyPermissions.mockResolvedValue([
      {
        name: "eq.view",
        description: "",
        category: "equipment",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    render(
      <PermissionGate permission="eq.view" fallback={<span>no eq.view</span>}>
        <span data-testid="gate-content">has eq.view</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("gate-content")).toBeInTheDocument();
    });
    expect(screen.getByText("has eq.view")).toBeInTheDocument();
    expect(screen.queryByText("no eq.view")).not.toBeInTheDocument();
  });

  it("renders fallback when user lacks the permission", async () => {
    mockGetMyPermissions.mockResolvedValue([
      {
        name: "di.view",
        description: "",
        category: "intervention",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    render(
      <PermissionGate permission="eq.view" fallback={<span>no eq.view</span>}>
        <span data-testid="gate-content">has eq.view</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("no eq.view")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("gate-content")).not.toBeInTheDocument();
  });

  it("renders nothing (null) during loading", () => {
    // Never resolve — stays in loading state
    mockGetMyPermissions.mockReturnValue(new Promise(() => {}));

    const { container } = render(
      <PermissionGate permission="eq.view">
        <span>should not appear</span>
      </PermissionGate>,
    );

    expect(container.innerHTML).toBe("");
  });

  it("renders nothing when no fallback and permission denied", async () => {
    mockGetMyPermissions.mockResolvedValue([]);

    const { container } = render(
      <PermissionGate permission="eq.view">
        <span>should not appear</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(container.innerHTML).toBe("");
    });
  });
});

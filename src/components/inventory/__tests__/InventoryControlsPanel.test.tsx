import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { InventoryControlsPanel } from "@/components/inventory/InventoryControlsPanel";

const mocks = vi.hoisted(() => ({
  approveInventoryCountLine: vi.fn(),
  createInventoryCountSession: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryCountLines: vi.fn(),
  listInventoryCountSessions: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryReconciliationFindings: vi.fn(),
  listInventoryReconciliationRuns: vi.fn(),
  listInventoryTransactions: vi.fn(),
  listInventoryWarehouses: vi.fn(),
  postInventoryCountSession: vi.fn(),
  reverseInventoryCountSession: vi.fn(),
  runInventoryReconciliation: vi.fn(),
  transitionInventoryCountSession: vi.fn(),
  upsertInventoryCountLine: vi.fn(),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 7 } }),
}));

vi.mock("@/services/inventory-service", () => mocks);

describe("InventoryControlsPanel", () => {
  beforeEach(() => {
    Object.values(mocks).forEach((fn) => fn.mockReset());
    mocks.listInventoryWarehouses.mockResolvedValue([{ id: 1, code: "MAIN", name: "Main", is_active: 1 }]);
    mocks.listInventoryLocations.mockResolvedValue([{ id: 10, warehouse_id: 1, warehouse_code: "MAIN", code: "BIN", is_active: 1 }]);
    mocks.listInventoryArticles.mockResolvedValue([{ id: 21, article_code: "A-21", article_name: "Bearing", is_active: 1 }]);
    mocks.listInventoryCountSessions.mockResolvedValue([]);
    mocks.listInventoryCountLines.mockResolvedValue([]);
    mocks.listInventoryReconciliationRuns.mockResolvedValue([]);
    mocks.listInventoryReconciliationFindings.mockResolvedValue([]);
    mocks.listInventoryTransactions.mockResolvedValue([]);
    mocks.createInventoryCountSession.mockResolvedValue({
      id: 100,
      session_code: "CC-100",
      warehouse_id: 1,
      location_id: 10,
      status: "draft",
      critical_abs_threshold: 5,
      submitted_by_id: null,
      submitted_at: null,
      posted_by_id: null,
      posted_at: null,
      reversed_by_id: null,
      reversed_at: null,
      reversal_reason: null,
      row_version: 1,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    });
  });

  it("creates a count session with governance threshold", async () => {
    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryWarehouses).toHaveBeenCalled());

    const selects = screen.getAllByRole("combobox");
    fireEvent.click(selects[0]!);
    fireEvent.click(screen.getByText("MAIN - Main"));
    fireEvent.click(selects[1]!);
    fireEvent.click(screen.getByText("MAIN/BIN"));
    fireEvent.change(screen.getByLabelText("Critical abs threshold"), { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "Create session" }));

    await waitFor(() => {
      expect(mocks.createInventoryCountSession).toHaveBeenCalledWith(
        expect.objectContaining({ warehouse_id: 1, location_id: 10, critical_abs_threshold: 3 }),
      );
    });
  });

  it("surfaces guarded failure when posting without reviewer evidence", async () => {
    mocks.listInventoryCountSessions.mockResolvedValue([
      {
        id: 100,
        session_code: "CC-100",
        warehouse_id: 1,
        location_id: 10,
        status: "approved",
        critical_abs_threshold: 5,
        submitted_by_id: null,
        submitted_at: null,
        posted_by_id: null,
        posted_at: null,
        reversed_by_id: null,
        reversed_at: null,
        reversal_reason: null,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.postInventoryCountSession.mockRejectedValue({ message: "cannot post without reviewer evidence" });

    render(<InventoryControlsPanel />);
    await waitFor(() => expect(mocks.listInventoryCountSessions).toHaveBeenCalled());

    const sessionSelect = screen.getAllByRole("combobox")[2]!;
    fireEvent.click(sessionSelect);
    fireEvent.click(screen.getByText("CC-100 (approved)"));
    fireEvent.click(screen.getByRole("button", { name: "Post variances" }));

    await waitFor(() => {
      expect(screen.getByText(/cannot post without reviewer evidence/i)).toBeInTheDocument();
    });
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ProcurementRepairablePanel } from "@/components/inventory/ProcurementRepairablePanel";

const mocks = vi.hoisted(() => ({
  createInventoryProcurementRequisition: vi.fn(),
  createInventoryPurchaseOrderFromRequisition: vi.fn(),
  createInventoryRepairableOrder: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryStockBalances: vi.fn(),
  listInventoryProcurementRequisitionLines: vi.fn(),
  listInventoryProcurementRequisitions: vi.fn(),
  listInventoryProcurementSuppliers: vi.fn(),
  listInventoryPurchaseOrderLines: vi.fn(),
  listInventoryPurchaseOrders: vi.fn(),
  listInventoryRepairableOrders: vi.fn(),
  listInventoryStateEvents: vi.fn(),
  receiveInventoryPurchaseOrderGoods: vi.fn(),
  transitionInventoryProcurementRequisition: vi.fn(),
  transitionInventoryPurchaseOrder: vi.fn(),
  transitionInventoryRepairableOrder: vi.fn(),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/services/inventory-service", () => mocks);

describe("ProcurementRepairablePanel", () => {
  beforeEach(() => {
    Object.values(mocks).forEach((fn) => fn.mockReset());
    mocks.listInventoryArticles.mockResolvedValue([
      { id: 11, article_code: "A-11", article_name: "Bearing", is_active: 1 },
    ]);
    mocks.listInventoryLocations.mockResolvedValue([
      { id: 101, warehouse_code: "MAIN", code: "BIN", is_active: 1 },
    ]);
    mocks.listInventoryStockBalances.mockResolvedValue([]);
    mocks.listInventoryProcurementSuppliers.mockResolvedValue([
      { id: 71, company_code: "SUP-01", company_name: "Supplier 1", is_active: 1 },
    ]);
    mocks.listInventoryProcurementRequisitions.mockResolvedValue([
      {
        id: 501,
        req_number: "REQ-1",
        demand_source_type: "REORDER",
        demand_source_id: 1,
        demand_source_ref: "R-1",
        status: "APPROVED",
        posting_state: "PENDING_POSTING",
        posting_error: null,
        requested_by_id: null,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.listInventoryProcurementRequisitionLines.mockResolvedValue([]);
    mocks.listInventoryPurchaseOrders.mockResolvedValue([
      {
        id: 601,
        po_number: "PO-1",
        requisition_id: 501,
        supplier_company_id: 71,
        supplier_company_name: "Supplier 1",
        status: "SUBMITTED",
        posting_state: "PENDING_POSTING",
        posting_error: null,
        ordered_by_id: null,
        ordered_at: null,
        approved_by_id: null,
        approved_at: null,
        row_version: 2,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.listInventoryPurchaseOrderLines.mockResolvedValue([
      {
        id: 801,
        purchase_order_id: 601,
        requisition_line_id: 1,
        article_id: 11,
        article_code: "A-11",
        article_name: "Bearing",
        ordered_qty: 5,
        received_qty: 0,
        unit_price: null,
        demand_source_type: "REORDER",
        demand_source_id: 1,
        demand_source_ref: "R-1",
        source_reservation_id: null,
        status: "OPEN",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.listInventoryRepairableOrders.mockResolvedValue([]);
    mocks.listInventoryStateEvents.mockResolvedValue([]);
    mocks.createInventoryProcurementRequisition.mockResolvedValue({});
    mocks.createInventoryPurchaseOrderFromRequisition.mockResolvedValue({});
    mocks.createInventoryRepairableOrder.mockResolvedValue({});
    mocks.receiveInventoryPurchaseOrderGoods.mockResolvedValue({});
    mocks.transitionInventoryProcurementRequisition.mockResolvedValue({});
    mocks.transitionInventoryPurchaseOrder.mockResolvedValue({});
    mocks.transitionInventoryRepairableOrder.mockResolvedValue({});
  });

  it("creates a requisition from demand inputs", async () => {
    render(<ProcurementRepairablePanel />);
    await waitFor(() => expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1));

    const selects = screen.getAllByRole("combobox");
    fireEvent.click(selects[0]!);
    fireEvent.click(screen.getByText("A-11 - Bearing"));
    fireEvent.click(selects[1]!);
    fireEvent.click(screen.getByText("MAIN/BIN"));
    fireEvent.change(screen.getByLabelText("Requested quantity"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Create requisition" }));

    await waitFor(() => {
      expect(mocks.createInventoryProcurementRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          article_id: 11,
          preferred_location_id: 101,
          requested_qty: 5,
        }),
      );
    });
  });

  it("shows guarded failure message on invalid PO transition", async () => {
    mocks.transitionInventoryPurchaseOrder.mockRejectedValue({ message: "Invalid lifecycle transition" });
    render(<ProcurementRepairablePanel />);
    await waitFor(() => expect(mocks.listInventoryPurchaseOrders).toHaveBeenCalledTimes(1));

    const poSelect = screen.getAllByRole("combobox")[6]!;
    fireEvent.click(poSelect);
    fireEvent.click(screen.getByText("PO-1 (SUBMITTED)"));
    fireEvent.click(screen.getByRole("button", { name: "Approve PO" }));

    await waitFor(() => {
      expect(screen.getByText(/Invalid lifecycle transition/i)).toBeInTheDocument();
    });
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  ProcurementRepairablePanel,
  purchaseOrderCancelTarget,
  requisitionDeclineOrCloseTarget,
  type ProcurementRepairablePanelHandle,
} from "@/components/inventory/ProcurementRepairablePanel";

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
  listInventorySuppliers: vi.fn(),
  getProcurementDashboardSummary: vi.fn(),
  getProcurementAlerts: vi.fn(),
  evaluateInventoryReplenishment: vi.fn(),
  listInventoryPurchaseOrderLines: vi.fn(),
  listInventoryPurchaseOrders: vi.fn(),
  listInventoryRepairableOrders: vi.fn(),
  listInventoryStateEvents: vi.fn(),
  receiveInventoryPurchaseOrderGoods: vi.fn(),
  transitionInventoryProcurementRequisition: vi.fn(),
  transitionInventoryPurchaseOrder: vi.fn(),
  transitionInventoryRepairableOrder: vi.fn(),
  listSupplierPrices: vi.fn(),
  listInventoryDocumentLinks: vi.fn(),
  getInventorySupplierScorecard: vi.fn(),
  listSupplierArticleSources: vi.fn(),
  upsertSupplierArticleSource: vi.fn(),
  listSupplierContacts: vi.fn(),
  upsertSupplierContact: vi.fn(),
  deleteSupplierContact: vi.fn(),
  listSupplierPurchaseHistory: vi.fn(),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/services/inventory-service", () => mocks);

describe("procurement transition targets", () => {
  it("maps requisition decline/close to backend FSM targets", () => {
    expect(requisitionDeclineOrCloseTarget("DRAFT")).toBe("CANCELLED");
    expect(requisitionDeclineOrCloseTarget("SUBMITTED")).toBe("CANCELLED");
    expect(requisitionDeclineOrCloseTarget("APPROVED")).toBe("CLOSED");
    expect(requisitionDeclineOrCloseTarget("PARTIALLY_RECEIVED")).toBe("CLOSED");
    expect(requisitionDeclineOrCloseTarget("CLOSED")).toBeNull();
    expect(requisitionDeclineOrCloseTarget("CANCELLED")).toBeNull();
  });

  it("maps PO cancel to CANCELLED never CLOSED", () => {
    expect(purchaseOrderCancelTarget("DRAFT")).toBe("CANCELLED");
    expect(purchaseOrderCancelTarget("SUBMITTED")).toBe("CANCELLED");
    expect(purchaseOrderCancelTarget("APPROVED")).toBe("CANCELLED");
    expect(purchaseOrderCancelTarget("PARTIALLY_RECEIVED")).toBe("CANCELLED");
    expect(purchaseOrderCancelTarget("RECEIVED_CLOSED")).toBeNull();
    expect(purchaseOrderCancelTarget("CANCELLED")).toBeNull();
  });
});

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
    mocks.listInventorySuppliers.mockResolvedValue([
      {
        id: 71,
        code: "SUP-01",
        name: "Supplier 1",
        status_code: "APPROVED",
        is_active: 1,
        external_company_id: 71,
        default_lead_time_days: 7,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.getProcurementDashboardSummary.mockResolvedValue({
      open_requisitions: 1,
      pending_approval_pos: 0,
      open_pos: 1,
      overdue_pos: 0,
      pending_receipts: 0,
      low_stock_articles: 0,
      critical_low_stock_articles: 0,
      repairables_in_repair: 0,
      active_suppliers_count: 1,
      receiving_today_count: 0,
    });
    mocks.getProcurementAlerts.mockResolvedValue([]);
    mocks.evaluateInventoryReplenishment.mockResolvedValue([]);
    mocks.listSupplierPrices.mockResolvedValue([]);
    mocks.listInventoryDocumentLinks.mockResolvedValue([]);
    mocks.getInventorySupplierScorecard.mockResolvedValue({
      supplier_id: 71,
      supplier_code: "SUP-01",
      supplier_name: "Supplier 1",
      on_time_delivery_pct: null,
      avg_lead_time_days: null,
      delivery_accuracy_pct: null,
      avg_price: null,
      open_po_count: 0,
      completed_po_count: 0,
      last_purchase_at: null,
      rejected_pct: null,
      risk_level: "LOW",
      last_deliveries: [],
    });
    mocks.listSupplierArticleSources.mockResolvedValue([]);
    mocks.upsertSupplierArticleSource.mockResolvedValue({});
    mocks.listSupplierContacts.mockResolvedValue([]);
    mocks.upsertSupplierContact.mockResolvedValue({});
    mocks.deleteSupplierContact.mockResolvedValue(undefined);
    mocks.listSupplierPurchaseHistory.mockResolvedValue([]);
    mocks.listInventoryProcurementRequisitions.mockResolvedValue([
      {
        id: 501,
        req_number: "REQ-1",
        demand_source_type: "REORDER",
        demand_source_id: 1,
        demand_source_ref: "R-1",
        purchase_priority: "NORMAL",
        status: "DRAFT",
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
        status: "DRAFT",
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
    mocks.listInventoryPurchaseOrderLines.mockResolvedValue([]);
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

  it("creates a requisition from the create dialog opened via ref", async () => {
    const ref = createRef<ProcurementRepairablePanelHandle>();
    render(<ProcurementRepairablePanel ref={ref} />);
    await waitFor(() => expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1));

    ref.current?.openCreateRequisition();
    await waitFor(() => expect(screen.getByText("New requisition")).toBeInTheDocument());

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

  it("cancels a draft requisition with CANCELLED not CLOSED", async () => {
    render(<ProcurementRepairablePanel />);
    await waitFor(() => expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByText("REQ-1"));
    await waitFor(() => expect(screen.getByRole("heading", { name: /REQ-1/ })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(mocks.transitionInventoryProcurementRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          requisition_id: 501,
          next_status: "CANCELLED",
        }),
      );
    });
  });

  it("cancels a draft PO with CANCELLED not CLOSED", async () => {
    render(<ProcurementRepairablePanel />);
    await waitFor(() => expect(mocks.listInventoryPurchaseOrders).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "Purchase orders" }));
    await waitFor(() => expect(screen.getByText("PO-1")).toBeInTheDocument());

    fireEvent.click(screen.getByText("PO-1"));
    await waitFor(() => expect(screen.getByRole("heading", { name: /PO-1/ })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(mocks.transitionInventoryPurchaseOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          purchase_order_id: 601,
          next_status: "CANCELLED",
        }),
      );
    });
  });

  it("exposes reload on the panel handle", async () => {
    const ref = createRef<ProcurementRepairablePanelHandle>();
    render(<ProcurementRepairablePanel ref={ref} />);
    await waitFor(() => expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1));

    await ref.current?.reload();
    await waitFor(() => expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(2));
  });
});

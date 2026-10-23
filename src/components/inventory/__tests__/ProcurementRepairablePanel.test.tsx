import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { createRef, type ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ProcurementRepairablePanel,
  purchaseOrderCancelTarget,
  requisitionDeclineOrCloseTarget,
  type ProcurementRepairablePanelHandle,
} from "@/components/inventory/ProcurementRepairablePanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";

const mocks = vi.hoisted(() => ({
  createInventoryMultiLineRequisition: vi.fn(),
  replaceInventoryRequisitionLines: vi.fn(),
  listCurrencyExchangeRates: vi.fn(),
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
  listInventoryGoodsReceipts: vi.fn(),
  listInventoryGoodsReceiptLines: vi.fn(),
  listInventoryStateEvents: vi.fn(),
  getInventoryPurchaseOrderDetail: vi.fn(),
  updateInventoryProcurementPostingState: vi.fn(),
  upsertInventoryDocumentLink: vi.fn(),
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

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => true,
    canAny: () => true,
    canAll: () => true,
    permissions: [],
    isLoading: false,
    reload: vi.fn(),
  }),
}));

vi.mock("@/services/inventory-service", () => mocks);
// Reference-data lists (demand sources, priorities, document purposes) come from the lookup
// service; labels used in assertions come from the i18n resources.
vi.mock("@/services/lookup-service", () => ({ getLookupValues: vi.fn().mockResolvedValue([]) }));

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

/**
 * Clicks a list row until its detail dialog opens: rows re-render while the reference-data
 * labels resolve, so a row element found once can be replaced before the click.
 */
async function openRow(text: string) {
  await waitFor(() => {
    fireEvent.click(screen.getByText(text));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
}

describe("ProcurementRepairablePanel", () => {
  // Real English resources so assertions read like the shipped UI.
  beforeAll(async () => {
    // Radix Select scrolls the selected option into view; jsdom has no layout.
    Element.prototype.scrollIntoView = vi.fn();
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory", "common"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory, common: enCommon } },
        interpolation: { escapeValue: false },
      });
    }
  });

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
        requested_by_name: null,
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
        ordered_by_name: null,
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
    mocks.listInventoryGoodsReceipts.mockResolvedValue([]);
    mocks.listInventoryGoodsReceiptLines.mockResolvedValue([]);
    mocks.listInventoryStateEvents.mockResolvedValue([]);
    mocks.listInventoryDocumentLinks.mockResolvedValue([]);
    mocks.updateInventoryProcurementPostingState.mockResolvedValue({});
    mocks.upsertInventoryDocumentLink.mockResolvedValue({});
    mocks.getInventoryPurchaseOrderDetail.mockImplementation(async (poId: number) => ({
      order: {
        id: poId,
        po_number: "PO-1",
        requisition_id: 501,
        supplier_company_id: 71,
        supplier_company_name: "Supplier 1",
        supplier_id: 71,
        supplier_name: "Supplier 1",
        status: "DRAFT",
        posting_state: "PENDING_POSTING",
        posting_error: null,
        ordered_by_id: null,
        ordered_by_name: null,
        ordered_at: null,
        approved_by_id: null,
        approved_at: null,
        expected_delivery_date: null,
        row_version: 2,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
      lines: [],
      goods_receipts: [],
      state_events: [],
      document_links: [],
      total_amount: null,
    }));
    mocks.createInventoryMultiLineRequisition.mockResolvedValue({});
    mocks.replaceInventoryRequisitionLines.mockResolvedValue({});
    mocks.listCurrencyExchangeRates.mockResolvedValue([]);
    mocks.createInventoryPurchaseOrderFromRequisition.mockResolvedValue({});
    mocks.createInventoryRepairableOrder.mockResolvedValue({});
    mocks.receiveInventoryPurchaseOrderGoods.mockResolvedValue({});
    mocks.transitionInventoryProcurementRequisition.mockResolvedValue({});
    mocks.transitionInventoryPurchaseOrder.mockResolvedValue({});
    mocks.transitionInventoryRepairableOrder.mockResolvedValue({});
  });

  it("creates a requisition from the create dialog opened via ref, then resets the form", async () => {
    const ref = createRef<ProcurementRepairablePanelHandle>();
    render(<ProcurementRepairablePanel ref={ref} />);
    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1),
    );

    act(() => ref.current?.openCreateRequisition());
    const dialog = await screen.findByRole("dialog", { name: "New purchase requisition" });

    fireEvent.click(within(dialog).getByLabelText("Article"));
    fireEvent.click(await screen.findByText("A-11 - Bearing"));
    fireEvent.click(within(dialog).getByLabelText("Delivery location"));
    fireEvent.click(await screen.findByText("MAIN/BIN"));
    fireEvent.change(within(dialog).getByLabelText("Requested quantity"), {
      target: { value: "5" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create requisition" }));

    await waitFor(() => {
      expect(mocks.createInventoryMultiLineRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          lines: [
            {
              article_id: 11,
              preferred_location_id: 101,
              requested_qty: 5,
              estimated_unit_cost: null,
            },
          ],
          demand_source_type: "REORDER",
        }),
      );
    });
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "New purchase requisition" })).toBeNull(),
    );

    // Opening the form again starts from an empty requisition.
    act(() => ref.current?.openCreateRequisition());
    const again = await screen.findByRole("dialog", { name: "New purchase requisition" });
    expect(within(again).getByLabelText("Requested quantity")).toHaveValue("0");
    expect(within(again).getByRole("button", { name: "Create requisition" })).toBeDisabled();
    expect(within(again).getByText("Choose the article to purchase.")).toBeInTheDocument();
  });

  it("pre-fills the article when the page asks for a requisition from an article", async () => {
    const onRequestHandled = vi.fn();
    render(
      <ProcurementRepairablePanel
        request={{ kind: "create-requisition", articleId: 11, locationId: 101 }}
        onRequestHandled={onRequestHandled}
      />,
    );

    const dialog = await screen.findByRole("dialog", { name: "New purchase requisition" });
    expect(onRequestHandled).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Article")).toHaveTextContent("A-11 - Bearing"),
    );
    expect(within(dialog).getByLabelText("Delivery location")).toHaveTextContent("MAIN/BIN");
    fireEvent.change(within(dialog).getByLabelText("Requested quantity"), {
      target: { value: "2" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create requisition" }));

    await waitFor(() => {
      expect(mocks.createInventoryMultiLineRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          lines: [
            expect.objectContaining({
              article_id: 11,
              preferred_location_id: 101,
              requested_qty: 2,
            }),
          ],
        }),
      );
    });
  });

  it("asks before cancelling a draft requisition and sends CANCELLED, not CLOSED", async () => {
    render(<ProcurementRepairablePanel />);
    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1),
    );

    await openRow("REQ-1");
    const detail = await screen.findByRole("dialog");
    await waitFor(() => expect(within(detail).getByText("REQ-1")).toBeInTheDocument());

    fireEvent.click(within(detail).getByRole("button", { name: "Cancel" }));
    expect(mocks.transitionInventoryProcurementRequisition).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "Cancel requisition" }));

    await waitFor(() => {
      expect(mocks.transitionInventoryProcurementRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          requisition_id: 501,
          next_status: "CANCELLED",
        }),
      );
    });
  });

  it("creates the purchase order from an approved requisition's detail", async () => {
    mocks.listInventoryProcurementRequisitions.mockResolvedValue([
      {
        id: 502,
        req_number: "REQ-2",
        demand_source_type: "REORDER",
        demand_source_id: null,
        demand_source_ref: null,
        purchase_priority: "HIGH",
        status: "APPROVED",
        posting_state: "PENDING_POSTING",
        posting_error: null,
        requested_by_id: 7,
        requested_by_name: "Amina Haddad",
        row_version: 3,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.createInventoryPurchaseOrderFromRequisition.mockResolvedValue({ id: 601 });

    render(<ProcurementRepairablePanel />);
    await openRow("REQ-2");
    const detail = await screen.findByRole("dialog");
    expect(within(detail).getByText("Amina Haddad")).toBeInTheDocument();
    expect(within(detail).getByText("Stock replenishment")).toBeInTheDocument();

    fireEvent.click(within(detail).getByRole("button", { name: "Create purchase order" }));
    const poDialog = await screen.findByRole("dialog", { name: "Create purchase order" });
    expect(within(poDialog).getByLabelText("Approved requisition")).toHaveTextContent("REQ-2");
    fireEvent.click(within(poDialog).getByRole("button", { name: "Create purchase order" }));

    await waitFor(() => {
      expect(mocks.createInventoryPurchaseOrderFromRequisition).toHaveBeenCalledWith(
        expect.objectContaining({ requisition_id: 502, supplier_id: null }),
      );
    });
  });

  it("keeps the rejection dialog open with the reason when the rejection fails", async () => {
    mocks.listInventoryProcurementRequisitions.mockResolvedValue([
      {
        id: 503,
        req_number: "REQ-3",
        demand_source_type: "MANUAL",
        demand_source_id: null,
        demand_source_ref: null,
        purchase_priority: "NORMAL",
        status: "SUBMITTED",
        posting_state: "PENDING_POSTING",
        posting_error: null,
        requested_by_id: null,
        requested_by_name: null,
        row_version: 1,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      },
    ]);
    mocks.transitionInventoryProcurementRequisition.mockRejectedValueOnce(
      new Error("stale row version"),
    );

    render(<ProcurementRepairablePanel />);
    await openRow("REQ-3");
    const detail = await screen.findByRole("dialog");
    fireEvent.click(within(detail).getByRole("button", { name: "Reject" }));

    const rejectDialog = await screen.findByRole("dialog", { name: "Reject requisition" });
    fireEvent.change(within(rejectDialog).getByLabelText("Rejection reason"), {
      target: { value: "Duplicate of REQ-2" },
    });
    fireEvent.click(within(rejectDialog).getByRole("button", { name: "Reject requisition" }));

    await waitFor(() =>
      expect(mocks.transitionInventoryProcurementRequisition).toHaveBeenCalledWith(
        expect.objectContaining({ next_status: "REJECTED", reason: "Duplicate of REQ-2" }),
      ),
    );
    const stillOpen = await screen.findByRole("dialog", { name: "Reject requisition" });
    expect(within(stillOpen).getByLabelText("Rejection reason")).toHaveValue("Duplicate of REQ-2");
  });

  it("asks before cancelling a draft PO and sends CANCELLED, not CLOSED", async () => {
    render(<ProcurementRepairablePanel />);
    await waitFor(() => expect(mocks.listInventoryPurchaseOrders).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "Purchase orders" }));
    await openRow("PO-1");
    const workspace = await screen.findByRole("dialog");
    await waitFor(() => expect(within(workspace).getByText("PO-1")).toBeInTheDocument());

    fireEvent.click(within(workspace).getByRole("button", { name: "Cancel" }));
    expect(mocks.transitionInventoryPurchaseOrder).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "Cancel purchase order" }));

    await waitFor(() => {
      expect(mocks.transitionInventoryPurchaseOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          purchase_order_id: 601,
          next_status: "CANCELLED",
        }),
      );
    });
  });

  it("shows an error with retry instead of empty lists when loading fails", async () => {
    mocks.listInventoryProcurementRequisitions.mockRejectedValueOnce(new Error("database locked"));

    render(<ProcurementRepairablePanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("database locked");
    expect(screen.queryByText("No requisitions yet.")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: enCommon.ds.state.retry }));

    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(2),
    );
    expect(await screen.findByText("REQ-1")).toBeInTheDocument();
  });

  it("exposes reload on the panel handle", async () => {
    const ref = createRef<ProcurementRepairablePanelHandle>();
    render(<ProcurementRepairablePanel ref={ref} />);
    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1),
    );

    await act(async () => {
      await ref.current?.reload();
    });
    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(2),
    );
  });

  it("creates a requisition with several lines, estimated costs and their total", async () => {
    mocks.listInventoryArticles.mockResolvedValue([
      { id: 11, article_code: "A-11", article_name: "Bearing", is_active: 1, avg_unit_cost: 12 },
      { id: 12, article_code: "A-12", article_name: "Seal kit", is_active: 1, avg_unit_cost: null },
    ]);
    const ref = createRef<ProcurementRepairablePanelHandle>();
    render(<ProcurementRepairablePanel ref={ref} />);
    await waitFor(() =>
      expect(mocks.listInventoryProcurementRequisitions).toHaveBeenCalledTimes(1),
    );

    act(() => ref.current?.openCreateRequisition());
    const dialog = await screen.findByRole("dialog", { name: "New purchase requisition" });

    // Line 1: the article's average cost is proposed as the estimate.
    fireEvent.click(within(dialog).getByLabelText("Article"));
    fireEvent.click(await screen.findByText("A-11 - Bearing"));
    fireEvent.change(within(dialog).getByLabelText("Requested quantity"), {
      target: { value: "2" },
    });
    expect(within(dialog).getByLabelText(/Estimated unit cost/)).toHaveValue("12,00");

    fireEvent.click(within(dialog).getByRole("button", { name: "Add a line" }));
    const line2 = within(dialog).getByRole("group", { name: "Line 2" });
    fireEvent.click(within(line2).getByLabelText("Article"));
    fireEvent.click(await screen.findByText("A-12 - Seal kit"));
    fireEvent.change(within(line2).getByLabelText("Requested quantity"), {
      target: { value: "3" },
    });
    expect(within(dialog).getByText(/some lines have no estimate/)).toBeInTheDocument();
    fireEvent.change(within(line2).getByLabelText(/Estimated unit cost/), {
      target: { value: "4,5" },
    });
    expect(within(dialog).queryByText(/some lines have no estimate/)).toBeNull();

    fireEvent.click(within(dialog).getByRole("button", { name: "Create requisition" }));
    await waitFor(() =>
      expect(mocks.createInventoryMultiLineRequisition).toHaveBeenCalledWith(
        expect.objectContaining({
          lines: [
            {
              article_id: 11,
              preferred_location_id: null,
              requested_qty: 2,
              estimated_unit_cost: 12,
            },
            {
              article_id: 12,
              preferred_location_id: null,
              requested_qty: 3,
              estimated_unit_cost: 4.5,
            },
          ],
        }),
      ),
    );
  });
});

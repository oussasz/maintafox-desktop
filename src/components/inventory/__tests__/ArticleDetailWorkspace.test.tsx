/**
 * ArticleDetailWorkspace — sections load independently (one failing call does not blank the
 * panel), codes are shown as labels, and equivalents use the backend's equivalence types.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { ArticleDetailWorkspace } from "@/components/inventory/ArticleDetailWorkspace";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import type { InventoryArticleRow } from "@/services/inventory-service";
import { useInventoryStore } from "@/stores/inventory-store";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  deleteInventoryArticleEquivalent: vi.fn(),
  getInventoryArticleRepairableHistory: vi.fn(),
  listInventoryArticleEquivalents: vi.fn(),
  listInventoryArticlePurchaseHistory: vi.fn(),
  listInventoryDocumentLinks: vi.fn(),
  listInventoryReservations: vi.fn(),
  listInventoryStateEvents: vi.fn(),
  listInventoryStockBalances: vi.fn(),
  listInventorySuppliers: vi.fn(),
  listInventoryTransactions: vi.fn(),
  upsertInventoryArticleEquivalent: vi.fn(),
  upsertInventoryDocumentLink: vi.fn(),
}));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});

vi.mock("@/services/lookup-service", () => ({
  getLookupValues: vi.fn(async (domain: string) => {
    const base = { color: null, is_active: 1 };
    if (domain === "inventory.equivalence_type") {
      return [
        {
          id: 1,
          code: "DIRECT",
          label: "Direct",
          fr_label: "Direct",
          en_label: "Direct replacement",
          ...base,
        },
        {
          id: 2,
          code: "FUNCTIONAL",
          label: "Fonctionnel",
          fr_label: "Fonctionnel",
          en_label: "Functional",
          ...base,
        },
      ];
    }
    return [];
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/components/inventory/SupplierArticleSourcesSection", () => ({
  SupplierArticleSourcesSection: () => <div data-testid="sources" />,
}));
vi.mock("@/components/inventory/SupplierComparisonCard", () => ({
  SupplierComparisonCard: () => <div data-testid="comparison" />,
}));
vi.mock("@/components/inventory/ArticleConsumptionChart", () => ({
  ArticleConsumptionChart: () => <div data-testid="consumption" />,
}));
vi.mock("@/components/inventory/InventoryAdjustmentDialog", () => ({
  InventoryAdjustmentDialog: () => null,
}));
vi.mock("@/components/detail/EntityQrCode", () => ({ EntityQrCode: () => null }));

const TS = "2026-01-01T00:00:00Z";

function article(overrides: Partial<InventoryArticleRow> = {}): InventoryArticleRow {
  return {
    id: 4,
    article_code: "ART-001",
    article_name: "Bearing 6204",
    family_id: null,
    family_code: null,
    family_name: null,
    unit_value_id: 10,
    unit_code: "EA",
    unit_label: "Each",
    unit_symbol: "ea",
    criticality_value_id: null,
    criticality_code: null,
    criticality_label: null,
    stocking_type_value_id: 11,
    stocking_type_code: "STOCK",
    stocking_type_label: "Stocked",
    tax_category_value_id: 12,
    tax_category_code: "STD",
    tax_category_label: "Standard",
    procurement_category_value_id: null,
    procurement_category_code: null,
    procurement_category_label: null,
    preferred_warehouse_id: null,
    preferred_warehouse_code: null,
    preferred_warehouse_name: null,
    preferred_location_id: null,
    preferred_location_code: null,
    preferred_location_name: null,
    min_stock: 2,
    max_stock: 20,
    reorder_point: 5,
    safety_stock: 1,
    manufacturer_name: null,
    manufacturer_part_number: null,
    oem_part_number: null,
    replenishment_policy_code: null,
    economic_order_qty: null,
    minimum_order_qty: null,
    maximum_order_qty: null,
    order_multiple_qty: null,
    lead_time_days: null,
    review_period_days: null,
    abc_class_code: null,
    xyz_class_code: null,
    is_critical_spare: 0,
    requires_expiration: 0,
    shelf_life_days: null,
    requires_batch_tracking: 0,
    is_active: 1,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    on_hand_qty: 12,
    reserved_qty: 2,
    available_qty: 10,
    avg_unit_cost: null,
    stock_value: null,
    unvalued_qty: 0,
    barcode: null,
    requires_serial_tracking: 0,
    ...overrides,
  };
}

function renderWorkspace() {
  return render(
    <ArticleDetailWorkspace
      article={article()}
      onEdit={vi.fn()}
      onDeactivate={vi.fn()}
      onCreateRequisition={vi.fn()}
    />,
  );
}

describe("ArticleDetailWorkspace", () => {
  beforeAll(async () => {
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
    Object.values(svc).forEach((fn) => fn.mockReset());
    svc.listInventoryStockBalances.mockResolvedValue([]);
    svc.listInventoryTransactions.mockResolvedValue([]);
    svc.listInventoryReservations.mockResolvedValue([]);
    svc.listInventoryStateEvents.mockResolvedValue([]);
    svc.listInventoryArticlePurchaseHistory.mockResolvedValue([]);
    svc.listInventoryArticleEquivalents.mockResolvedValue([]);
    svc.getInventoryArticleRepairableHistory.mockResolvedValue([]);
    svc.listInventoryDocumentLinks.mockResolvedValue([]);
    svc.listInventorySuppliers.mockResolvedValue([]);
    useInventoryStore.setState({
      articles: [
        article(),
        article({ id: 9, article_code: "ART-009", article_name: "Bearing 6204-Z" }),
      ],
    });
  });

  it("keeps the panel usable when one section fails, with a retry in that section", async () => {
    svc.listInventoryArticlePurchaseHistory.mockRejectedValueOnce(new Error("history unavailable"));
    svc.listInventoryArticlePurchaseHistory.mockResolvedValue([
      {
        purchase_order_id: 77,
        po_number: "PO-77",
        ordered_at: TS,
        supplier_id: 1,
        supplier_name: "Roulements SA",
        unit_price: 12.5,
        ordered_qty: 4,
        received_qty: 4,
        status: "RECEIVED_CLOSED",
      },
    ]);

    renderWorkspace();

    // The header and the details tab render even though one call failed.
    expect((await screen.findAllByText("Bearing 6204")).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Purchase history" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("history unavailable");

    fireEvent.click(screen.getByRole("button", { name: enCommon.ds.state.retry }));
    expect(await screen.findByText("PO-77")).toBeInTheDocument();
    // The PO status is a translated chip, never the raw code.
    expect(screen.getByText("Received & closed")).toBeInTheDocument();
    expect(screen.queryByText("RECEIVED_CLOSED")).toBeNull();
  });

  it("adds an equivalent with a backend equivalence type and removes it only after confirmation", async () => {
    svc.upsertInventoryArticleEquivalent.mockResolvedValue({});
    svc.listInventoryArticleEquivalents.mockResolvedValue([
      {
        id: 55,
        article_id: 4,
        article_code: "ART-001",
        article_name: "Bearing 6204",
        equivalent_article_id: 9,
        equivalent_code: "ART-009",
        equivalent_name: "Bearing 6204-Z",
        equivalence_type: "FUNCTIONAL",
        notes: null,
        is_bidirectional: 1,
        created_at: TS,
      },
    ]);
    svc.deleteInventoryArticleEquivalent.mockResolvedValue(undefined);

    renderWorkspace();
    fireEvent.click(await screen.findByRole("button", { name: "Equivalents" }));
    expect(await screen.findByText("Functional")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Add equivalent" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByLabelText("Equivalent article"));
    fireEvent.click(await screen.findByRole("option", { name: "ART-009 — Bearing 6204-Z" }));
    fireEvent.click(within(dialog).getByRole("button", { name: enCommon.action.save }));

    await waitFor(() =>
      expect(svc.upsertInventoryArticleEquivalent).toHaveBeenCalledWith(
        expect.objectContaining({
          article_id: 4,
          equivalent_article_id: 9,
          equivalence_type: "DIRECT",
        }),
      ),
    );

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(svc.deleteInventoryArticleEquivalent).not.toHaveBeenCalled();
    const confirm = await screen.findByRole("dialog", { name: "Remove this equivalent?" });
    fireEvent.click(within(confirm).getByRole("button", { name: "Remove equivalent" }));
    await waitFor(() => expect(svc.deleteInventoryArticleEquivalent).toHaveBeenCalledWith(55));
  });
});

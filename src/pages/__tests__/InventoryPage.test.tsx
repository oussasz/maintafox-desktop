/**
 * InventoryPage — module shell, article stock columns, load failure, deactivation confirm,
 * "create requisition from article" hand-off and ABC/XYZ recalculation feedback.
 * The inventory-service IPC boundary is mocked; heavy tab panels are stubbed.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import { InventoryPage } from "@/pages/InventoryPage";
import type * as InventoryService from "@/services/inventory-service";
import type { InventoryArticleRow } from "@/services/inventory-service";
import { useAppToastStore } from "@/store/app-toast-store";
import { useInventoryStore } from "@/stores/inventory-store";
import { useLocaleStore } from "@/stores/locale-store";
import { formatCurrency } from "@/utils/formatters";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  listInventoryArticleFamilies: vi.fn(),
  listInventoryWarehouses: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryArticles: vi.fn(),
  createInventoryArticle: vi.fn(),
  updateInventoryArticle: vi.fn(),
  calculateInventoryAbc: vi.fn(),
  calculateInventoryXyz: vi.fn(),
}));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});

vi.mock("@/services/lookup-service", () => ({
  getLookupValues: vi.fn(async (domain: string) => {
    const base = { fr_label: null, en_label: null, color: null, is_active: 1 };
    if (domain === "inventory.stocking_type")
      return [{ id: 11, code: "STOCK", label: "Stocked", ...base }];
    if (domain === "inventory.tax_category")
      return [{ id: 12, code: "STD", label: "Standard", ...base }];
    return [];
  }),
}));

// Stock units come from the governed reference list MEASURE.UNITS.
vi.mock("@/services/reference-service", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return {
    ...actual,
    listPublishedReferenceValuesByDomainCode: vi.fn(async (code: string) =>
      code === "MEASURE.UNITS"
        ? [
            {
              id: 10,
              set_id: 1,
              parent_id: null,
              code: "EA",
              label: "Each",
              description: null,
              sort_order: 1,
              color_hex: null,
              icon_name: null,
              semantic_tag: "count",
              external_code: "H87",
              is_active: true,
              metadata_json: '{"origin":"system","symbol":"ea","label_en":"Each"}',
            },
          ]
        : [],
    ),
  };
});

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

const procurementProps = vi.hoisted(() => ({ last: null as Record<string, unknown> | null }));

vi.mock("@/components/inventory/ArticleDetailWorkspace", () => ({
  ArticleDetailWorkspace: (props: {
    article: { article_code: string };
    onEdit: () => void;
    onDeactivate: () => void;
    onCreateRequisition: () => void;
  }) => (
    <div data-testid="article-detail">
      <span>detail {props.article.article_code}</span>
      <button type="button" onClick={props.onEdit}>
        stub-edit
      </button>
      <button type="button" onClick={props.onDeactivate}>
        stub-deactivate
      </button>
      <button type="button" onClick={props.onCreateRequisition}>
        stub-requisition
      </button>
    </div>
  ),
}));

vi.mock("@/components/inventory/ProcurementRepairablePanel", async () => {
  const { forwardRef } = await import("react");
  return {
    ProcurementRepairablePanel: forwardRef(function Stub(props: Record<string, unknown>) {
      procurementProps.last = props;
      return <div data-testid="procurement-panel" />;
    }),
  };
});
vi.mock("@/components/inventory/InventoryControlsPanel", () => ({
  InventoryControlsPanel: () => <div data-testid="controls-panel" />,
}));
vi.mock("@/components/inventory/UnitConversionsPanel", () => ({
  UnitConversionsPanel: () => <div data-testid="units-panel" />,
}));
vi.mock("@/components/inventory/WarehouseLocationPanel", async () => {
  const { forwardRef } = await import("react");
  return {
    WarehouseLocationPanel: forwardRef(function Stub() {
      return <div data-testid="topology-panel" />;
    }),
  };
});

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
    preferred_warehouse_id: 2,
    preferred_warehouse_code: "WH1",
    preferred_warehouse_name: "Main",
    preferred_location_id: 3,
    preferred_location_code: "A-01",
    preferred_location_name: "Aisle A",
    min_stock: 2,
    max_stock: 20,
    reorder_point: 5,
    safety_stock: 1,
    manufacturer_name: "SKF",
    manufacturer_part_number: "6204-2RS",
    oem_part_number: null,
    replenishment_policy_code: null,
    economic_order_qty: null,
    minimum_order_qty: null,
    maximum_order_qty: null,
    order_multiple_qty: null,
    lead_time_days: 14,
    review_period_days: null,
    abc_class_code: null,
    xyz_class_code: null,
    is_critical_spare: 1,
    requires_expiration: 0,
    shelf_life_days: null,
    requires_batch_tracking: 0,
    is_active: 1,
    row_version: 3,
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
    reorder_trigger: null,
    ...overrides,
  };
}

const initialStore = useInventoryStore.getState();

function renderPage(path = "/inventory") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <InventoryPage />
    </MemoryRouter>,
  );
}

function primeLists(articles: InventoryArticleRow[]) {
  svc.listInventoryArticleFamilies.mockResolvedValue([]);
  svc.listInventoryWarehouses.mockResolvedValue([]);
  svc.listInventoryLocations.mockResolvedValue([]);
  svc.listInventoryArticles.mockResolvedValue(articles);
}

describe("InventoryPage", () => {
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
    useInventoryStore.setState(initialStore, true);
    useAppToastStore.setState({ items: [] });
    procurementProps.last = null;
  });

  it("renders the module shell with translated title, article count and stock columns", async () => {
    primeLists([
      article(),
      article({
        id: 5,
        article_code: "ART-002",
        article_name: "Seal kit",
        on_hand_qty: 4,
        reserved_qty: 1,
        available_qty: 3,
        avg_unit_cost: 12.5,
        stock_value: 50,
        unvalued_qty: 0,
        // The backend's low-stock rule (evaluate_reorder) flagged it.
        reorder_trigger: "reorder_point",
      }),
    ]);

    renderPage();

    const heading = screen.getByRole("heading", { name: "Inventory & procurement" });
    expect(await screen.findByText("ART-002")).toBeInTheDocument();
    // Count badge next to the title.
    await waitFor(() =>
      expect(heading.parentElement).toHaveTextContent(/^Inventory & procurement2$/),
    );
    for (const header of [
      "On hand",
      "Reserved",
      "Available",
      "Avg unit cost",
      "Stock value",
      "Reorder point",
    ]) {
      expect(screen.getByText(header)).toBeInTheDocument();
    }
    // ART-002 has 3 available for a reorder point of 5: flagged, ART-001 (10) is not.
    const lowRow = screen.getByText("ART-002").closest("tr");
    const okRow = screen.getByText("ART-001").closest("tr");
    if (!lowRow || !okRow) throw new Error("expected article rows");
    expect(within(lowRow).getAllByText("Reorder").length).toBeGreaterThan(0);
    expect(within(okRow).queryByText("Reorder")).toBeNull();
    expect(screen.getByRole("button", { name: enCommon.ds.toolbar.refresh })).toBeInTheDocument();
    // Valuation in the tenant currency and app locale (same formatter as the screen).
    const { activeLocale, activeCurrencyCode } = useLocaleStore.getState();
    const money = (v: number) => formatCurrency(v, activeLocale, activeCurrencyCode);
    expect(lowRow.textContent).toContain(money(12.5));
    expect(lowRow.textContent).toContain(money(50));
    // ART-001 was never costed: no invented value.
    expect(within(okRow).getAllByText("—").length).toBeGreaterThan(0);
  });

  it("flags an article whose total on hand is negative with a text badge, and not a healthy one", async () => {
    primeLists([
      article(),
      article({
        id: 6,
        article_code: "ART-NEG",
        article_name: "Oversold seal",
        on_hand_qty: -2,
        reserved_qty: 0,
        available_qty: -2,
      }),
    ]);

    renderPage();

    const negativeRow = (await screen.findByText("ART-NEG")).closest("tr");
    const healthyRow = screen.getByText("ART-001").closest("tr");
    if (!negativeRow || !healthyRow) throw new Error("expected article rows");
    const badge = within(negativeRow).getByText("Negative stock");
    expect(badge.closest("span[title]")).toHaveAttribute(
      "title",
      "Total over all stores is negative. To be corrected: another PC may have recorded a movement. Record a count in “Stock to reconcile” (Inventory › Controls).",
    );
    expect(within(healthyRow).queryByText("Negative stock")).toBeNull();

    // The chip is also a status of the list's stock filter.
    Element.prototype.scrollIntoView = vi.fn();
    const stockFilter = screen.getAllByRole("combobox")[1];
    if (!stockFilter) throw new Error("expected the stock filter");
    fireEvent.click(stockFilter);
    fireEvent.click(await screen.findByRole("option", { name: "Negative stock" }));
    await waitFor(() => expect(screen.queryByText("ART-001")).toBeNull());
    expect(screen.getByText("ART-NEG")).toBeInTheDocument();
  });

  it("opens the article named in ?articleId= (link from another module)", async () => {
    primeLists([article(), article({ id: 5, article_code: "ART-002", article_name: "Seal kit" })]);

    renderPage("/inventory?articleId=5");

    expect(await screen.findByText("detail ART-002")).toBeInTheDocument();
    expect(screen.queryByText("detail ART-001")).toBeNull();
  });

  it("clears the article search when it hides the linked article", async () => {
    primeLists([article()]);
    useInventoryStore.setState({ articleSearch: "seal" });
    svc.listInventoryArticles.mockImplementation(async ({ search }: { search: string | null }) =>
      search ? [] : [article()],
    );

    renderPage("/inventory?articleId=4");

    expect(await screen.findByText("detail ART-001")).toBeInTheDocument();
    expect(useInventoryStore.getState().articleSearch).toBe("");
  });

  it("shows an error with retry instead of an empty list when the articles cannot load", async () => {
    primeLists([]);
    svc.listInventoryArticles.mockRejectedValueOnce(new Error("database is locked"));

    renderPage();

    expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
    svc.listInventoryArticles.mockResolvedValue([article()]);
    fireEvent.click(screen.getByRole("button", { name: enCommon.ds.state.retry }));
    expect(await screen.findByText("ART-001")).toBeInTheDocument();
  });

  it("deactivates an article only after confirmation, keeping its item-master data", async () => {
    primeLists([article()]);
    svc.updateInventoryArticle.mockResolvedValue(article({ is_active: 0 }));

    renderPage();
    fireEvent.click(await screen.findByText("ART-001"));
    fireEvent.click(await screen.findByRole("button", { name: "stub-deactivate" }));
    expect(svc.updateInventoryArticle).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("dialog", { name: "Deactivate this article?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Deactivate article" }));

    await waitFor(() =>
      expect(svc.updateInventoryArticle).toHaveBeenCalledWith(
        4,
        3,
        expect.objectContaining({
          is_active: false,
          manufacturer_name: "SKF",
          manufacturer_part_number: "6204-2RS",
          lead_time_days: 14,
          is_critical_spare: true,
        }),
      ),
    );
  });

  it("hands the selected article to the procurement panel to pre-fill a requisition", async () => {
    primeLists([article()]);

    renderPage();
    fireEvent.click(await screen.findByText("ART-001"));
    fireEvent.click(await screen.findByRole("button", { name: "stub-requisition" }));

    await waitFor(() =>
      expect(procurementProps.last?.["request"]).toEqual({
        kind: "create-requisition",
        articleId: 4,
        locationId: 3,
      }),
    );
    act(() => (procurementProps.last?.["onRequestHandled"] as () => void)());
    await waitFor(() => expect(procurementProps.last?.["request"]).toBeNull());
  });

  it("reports the ABC recalculation and reloads the edited article's class", async () => {
    primeLists([article()]);
    svc.calculateInventoryAbc.mockResolvedValue(4);

    renderPage();
    fireEvent.click(await screen.findByText("ART-001"));
    fireEvent.click(await screen.findByRole("button", { name: "stub-edit" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit article" });

    svc.listInventoryArticles.mockResolvedValue([article({ abc_class_code: "A" })]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Calculate ABC" }));

    await waitFor(() => expect(svc.calculateInventoryAbc).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(useAppToastStore.getState().items.map((i) => i.title)).toContain(
        "ABC class recalculated for 4 articles",
      ),
    );
    await waitFor(() => expect(within(dialog).getByLabelText("ABC class")).toHaveTextContent("A"));
  });
});

/**
 * Behaviour tests for inventory-store (article master data + reference lists).
 * Only the inventory-service boundary is mocked.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  type InventoryArticleRow,
  createInventoryArticle,
  listInventoryArticleFamilies,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryWarehouses,
  updateInventoryArticle,
} from "@/services/inventory-service";
import type {
  ArticleFamily,
  InventoryArticleInput,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";

import { useInventoryStore } from "../inventory-store";

vi.mock("@/services/inventory-service", () => ({
  createInventoryArticle: vi.fn(),
  listInventoryArticleFamilies: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryLocations: vi.fn(),
  listInventoryWarehouses: vi.fn(),
  updateInventoryArticle: vi.fn(),
}));

const mockCreate = vi.mocked(createInventoryArticle);
const mockFamilies = vi.mocked(listInventoryArticleFamilies);
const mockArticles = vi.mocked(listInventoryArticles);
const mockLocations = vi.mocked(listInventoryLocations);
const mockWarehouses = vi.mocked(listInventoryWarehouses);
const mockUpdate = vi.mocked(updateInventoryArticle);

const TS = "2026-01-01T00:00:00Z";

function makeFamily(overrides: Partial<ArticleFamily> = {}): ArticleFamily {
  return {
    id: 1,
    code: "BRG",
    name: "Bearings",
    description: null,
    is_active: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function makeWarehouse(overrides: Partial<Warehouse> = {}): Warehouse {
  return { id: 2, code: "WH1", name: "Main", is_active: 1, created_at: TS, ...overrides };
}

function makeLocation(overrides: Partial<StockLocation> = {}): StockLocation {
  return {
    id: 3,
    warehouse_id: 2,
    warehouse_code: "WH1",
    code: "A-01",
    name: "Aisle A",
    is_default: 1,
    is_active: 1,
    created_at: TS,
    updated_at: TS,
    row_version: 1,
    ...overrides,
  };
}

function makeArticle(overrides: Partial<InventoryArticleRow> = {}): InventoryArticleRow {
  return {
    id: 4,
    article_code: "ART-001",
    article_name: "Bearing 6204",
    family_id: 1,
    family_code: "BRG",
    family_name: "Bearings",
    unit_value_id: 10,
    unit_code: "EA",
    unit_label: "Each",
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
    ...overrides,
  };
}

function makeInput(overrides: Partial<InventoryArticleInput> = {}): InventoryArticleInput {
  return {
    article_code: "ART-001",
    article_name: "Bearing 6204",
    family_id: 1,
    unit_value_id: 10,
    stocking_type_value_id: 11,
    tax_category_value_id: 12,
    min_stock: 2,
    max_stock: 20,
    reorder_point: 5,
    safety_stock: 1,
    is_active: true,
    ...overrides,
  };
}

function primeLists(articles: InventoryArticleRow[] = [makeArticle()]) {
  mockFamilies.mockResolvedValue([makeFamily()]);
  mockWarehouses.mockResolvedValue([makeWarehouse()]);
  mockLocations.mockResolvedValue([makeLocation()]);
  mockArticles.mockResolvedValue(articles);
}

const initial = useInventoryStore.getState();

beforeEach(() => {
  vi.resetAllMocks();
  useInventoryStore.setState(initial, true);
});

describe("inventory-store", () => {
  it("starts empty", () => {
    const s = useInventoryStore.getState();
    expect(s.families).toEqual([]);
    expect(s.warehouses).toEqual([]);
    expect(s.locations).toEqual([]);
    expect(s.articles).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
    expect(s.articleSearch).toBe("");
  });

  describe("loadAll", () => {
    it("loads all four lists, with null search when the search is empty", async () => {
      primeLists();
      let resolveFamilies: (v: ArticleFamily[]) => void = () => undefined;
      mockFamilies.mockReturnValueOnce(
        new Promise<ArticleFamily[]>((r) => {
          resolveFamilies = r;
        }),
      );
      useInventoryStore.setState({ error: "old" });

      const p = useInventoryStore.getState().loadAll();
      expect(useInventoryStore.getState().loading).toBe(true);
      expect(useInventoryStore.getState().error).toBeNull();

      resolveFamilies([makeFamily()]);
      await p;

      expect(mockLocations).toHaveBeenCalledWith(null);
      expect(mockArticles).toHaveBeenCalledWith({ search: null });
      const s = useInventoryStore.getState();
      expect(s.families).toEqual([makeFamily()]);
      expect(s.warehouses).toEqual([makeWarehouse()]);
      expect(s.locations).toEqual([makeLocation()]);
      expect(s.articles).toEqual([makeArticle()]);
      expect(s.loading).toBe(false);
    });

    it("clears a previous load failure once a load succeeds", async () => {
      primeLists();
      useInventoryStore.setState({ loadError: new Error("old failure") });

      await useInventoryStore.getState().loadAll();

      expect(useInventoryStore.getState().loadError).toBeNull();
      expect(useInventoryStore.getState().articles[0]?.available_qty).toBe(10);
    });

    it("passes the current articleSearch to listInventoryArticles", async () => {
      primeLists();
      useInventoryStore.setState({ articleSearch: "bearing" });

      await useInventoryStore.getState().loadAll();

      expect(mockArticles).toHaveBeenCalledWith({ search: "bearing" });
    });

    it("swallows a failure of any list, sets error and keeps previous data", async () => {
      primeLists();
      const failure = new Error("warehouses failed");
      mockWarehouses.mockRejectedValueOnce(failure);
      const previous = [makeArticle({ id: 99 })];
      useInventoryStore.setState({ articles: previous });

      await expect(useInventoryStore.getState().loadAll()).resolves.toBeUndefined();

      const s = useInventoryStore.getState();
      expect(s.error).toBe("warehouses failed");
      // The raw failure is kept for ErrorState (translated by describeAppError).
      expect(s.loadError).toBe(failure);
      expect(s.articles).toBe(previous);
      expect(s.families).toEqual([]);
      expect(s.loading).toBe(false);
    });
  });

  it("setArticleSearch stores the search and reloads with it", async () => {
    primeLists([makeArticle({ id: 7, article_code: "FLT-1" })]);

    await useInventoryStore.getState().setArticleSearch("flt");

    const s = useInventoryStore.getState();
    expect(s.articleSearch).toBe("flt");
    expect(mockArticles).toHaveBeenCalledWith({ search: "flt" });
    expect(s.articles).toEqual([makeArticle({ id: 7, article_code: "FLT-1" })]);
  });

  describe("createArticle", () => {
    it("creates, then reloads all lists; saving toggles", async () => {
      primeLists();
      const input = makeInput();
      let resolveCreate: (v: InventoryArticleRow) => void = () => undefined;
      mockCreate.mockReturnValueOnce(
        new Promise<InventoryArticleRow>((r) => {
          resolveCreate = r;
        }),
      );

      const p = useInventoryStore.getState().createArticle(input);
      expect(useInventoryStore.getState().saving).toBe(true);
      expect(mockArticles).not.toHaveBeenCalled();

      resolveCreate(makeArticle());
      await p;

      expect(mockCreate).toHaveBeenCalledWith(input);
      expect(mockArticles).toHaveBeenCalledTimes(1);
      expect(useInventoryStore.getState().articles).toEqual([makeArticle()]);
      expect(useInventoryStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure, without reloading", async () => {
      const err = { code: "VALIDATION", message: "duplicate code" };
      mockCreate.mockRejectedValueOnce(err);

      await expect(useInventoryStore.getState().createArticle(makeInput())).rejects.toBe(err);

      expect(mockArticles).not.toHaveBeenCalled();
      expect(useInventoryStore.getState().error).toBe("duplicate code");
      expect(useInventoryStore.getState().saving).toBe(false);
    });
  });

  describe("updateArticle", () => {
    it("passes id, row version and input, then reloads", async () => {
      primeLists();
      mockUpdate.mockResolvedValueOnce(makeArticle({ row_version: 2 }));
      const input = makeInput({ article_name: "Renamed" });

      await useInventoryStore.getState().updateArticle(4, 1, input);

      expect(mockUpdate).toHaveBeenCalledWith(4, 1, input);
      expect(mockArticles).toHaveBeenCalledTimes(1);
      expect(useInventoryStore.getState().saving).toBe(false);
      expect(useInventoryStore.getState().error).toBeNull();
    });

    it("sets error and rethrows on failure", async () => {
      mockUpdate.mockRejectedValueOnce(new Error("version conflict"));

      await expect(useInventoryStore.getState().updateArticle(4, 1, makeInput())).rejects.toThrow(
        "version conflict",
      );

      expect(mockArticles).not.toHaveBeenCalled();
      expect(useInventoryStore.getState().error).toBe("version conflict");
      expect(useInventoryStore.getState().saving).toBe(false);
    });

    it("succeeds even when the follow-up reload fails (reload error is swallowed into error)", async () => {
      primeLists();
      mockFamilies.mockRejectedValueOnce(new Error("reload failed"));
      mockUpdate.mockResolvedValueOnce(makeArticle());

      await expect(
        useInventoryStore.getState().updateArticle(4, 1, makeInput()),
      ).resolves.toBeUndefined();

      expect(useInventoryStore.getState().error).toBe("reload failed");
      expect(useInventoryStore.getState().saving).toBe(false);
    });
  });
});

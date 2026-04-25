import { create } from "zustand";

import {
  adjustInventoryStock,
  createInventoryArticle,
  listInventoryArticleFamilies,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryStockBalances,
  listInventoryWarehouses,
  updateInventoryArticle,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  ArticleFamily,
  InventoryArticle,
  InventoryArticleInput,
  InventoryStockAdjustInput,
  InventoryStockBalance,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";

interface InventoryStoreState {
  families: ArticleFamily[];
  warehouses: Warehouse[];
  locations: StockLocation[];
  articles: InventoryArticle[];
  balances: InventoryStockBalance[];
  loading: boolean;
  saving: boolean;
  error: string | null;
  selectedWarehouseId: number | null;
  lowStockOnly: boolean;
  articleSearch: string;

  loadAll: () => Promise<void>;
  setWarehouse: (warehouseId: number | null) => Promise<void>;
  setLowStockOnly: (enabled: boolean) => Promise<void>;
  setArticleSearch: (search: string) => Promise<void>;
  createArticle: (input: InventoryArticleInput) => Promise<void>;
  updateArticle: (id: number, rowVersion: number, input: InventoryArticleInput) => Promise<void>;
  adjustStock: (input: InventoryStockAdjustInput) => Promise<void>;
}

export const useInventoryStore = create<InventoryStoreState>()((set, get) => ({
  families: [],
  warehouses: [],
  locations: [],
  articles: [],
  balances: [],
  loading: false,
  saving: false,
  error: null,
  selectedWarehouseId: null,
  lowStockOnly: false,
  articleSearch: "",

  loadAll: async () => {
    set({ loading: true, error: null });
    try {
      const { selectedWarehouseId, lowStockOnly, articleSearch } = get();
      const [families, warehouses, locations, articles, balances] = await Promise.all([
        listInventoryArticleFamilies(),
        listInventoryWarehouses(),
        listInventoryLocations(selectedWarehouseId),
        listInventoryArticles({ search: articleSearch || null }),
        listInventoryStockBalances({
          warehouse_id: selectedWarehouseId,
          low_stock_only: lowStockOnly,
        }),
      ]);
      set({ families, warehouses, locations, articles, balances });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  setWarehouse: async (warehouseId) => {
    set({ selectedWarehouseId: warehouseId });
    await get().loadAll();
  },

  setLowStockOnly: async (enabled) => {
    set({ lowStockOnly: enabled });
    await get().loadAll();
  },

  setArticleSearch: async (search) => {
    set({ articleSearch: search });
    await get().loadAll();
  },

  createArticle: async (input) => {
    set({ saving: true, error: null });
    try {
      await createInventoryArticle(input);
      await get().loadAll();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updateArticle: async (id, rowVersion, input) => {
    set({ saving: true, error: null });
    try {
      await updateInventoryArticle(id, rowVersion, input);
      await get().loadAll();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  adjustStock: async (input) => {
    set({ saving: true, error: null });
    try {
      await adjustInventoryStock(input);
      await get().loadAll();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));

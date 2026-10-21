import { create } from "zustand";

import {
  type InventoryArticleRow,
  createInventoryArticle,
  listInventoryArticleFamilies,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryWarehouses,
  updateInventoryArticle,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  ArticleFamily,
  InventoryArticleInput,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";

interface InventoryStoreState {
  families: ArticleFamily[];
  warehouses: Warehouse[];
  locations: StockLocation[];
  articles: InventoryArticleRow[];
  loading: boolean;
  saving: boolean;
  /** Last failure (load or save) as text, kept for callers that show it inline. */
  error: string | null;
  /** Raw failure of the last `loadAll` (for `ErrorState`); null once a load succeeds. */
  loadError: unknown;
  articleSearch: string;

  loadAll: () => Promise<void>;
  setArticleSearch: (search: string) => Promise<void>;
  createArticle: (input: InventoryArticleInput) => Promise<void>;
  updateArticle: (id: number, rowVersion: number, input: InventoryArticleInput) => Promise<void>;
}

export const useInventoryStore = create<InventoryStoreState>()((set, get) => ({
  families: [],
  warehouses: [],
  locations: [],
  articles: [],
  loading: false,
  saving: false,
  error: null,
  loadError: null,
  articleSearch: "",

  loadAll: async () => {
    set({ loading: true, error: null });
    try {
      const { articleSearch } = get();
      // Stock balances are fetched per context (adjustment dialog, article detail, dashboards).
      // listInventoryStockBalances() remains available in inventory-service.
      const [families, warehouses, locations, articles] = await Promise.all([
        listInventoryArticleFamilies(),
        listInventoryWarehouses(),
        listInventoryLocations(null),
        listInventoryArticles({ search: articleSearch || null }),
      ]);
      set({ families, warehouses, locations, articles, loadError: null });
    } catch (err) {
      set({ error: toErrorMessage(err), loadError: err });
    } finally {
      set({ loading: false });
    }
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
}));

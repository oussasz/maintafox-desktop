/**
 * asset-search-store.ts
 *
 * Zustand store for asset search query state and result management.
 * Maintains filter criteria, result list, and selected result independently
 * from the main asset-store (which manages CRUD and hierarchy context).
 */

import { create } from "zustand";

import {
  searchAssets,
  suggestAssetCodes,
  suggestAssetNames,
} from "@/services/asset-search-service";
import { toErrorMessage } from "@/utils/errors";
import type { AssetSearchFilters, AssetSearchResult, AssetSuggestion } from "@shared/ipc-types";

// ── Default filter state ──────────────────────────────────────────────────────

const DEFAULT_FILTERS: AssetSearchFilters = {
  query: null,
  classCodes: null,
  familyCodes: null,
  statusCodes: null,
  orgNodeIds: null,
  includeDecommissioned: false,
  limit: 100,
};

// ── Store interface ───────────────────────────────────────────────────────────

interface AssetSearchStoreState {
  filters: AssetSearchFilters;
  results: AssetSearchResult[];
  selectedResultId: number | null;
  loading: boolean;
  error: string | null;

  /** Execute search with current filters. */
  runSearch: () => Promise<void>;
  /** Merge partial filter updates and re-execute search. */
  updateFilters: (partial: Partial<AssetSearchFilters>) => Promise<void>;
  /** Reset all filters to defaults and re-execute search. */
  clearFilters: () => Promise<void>;
  /** Set the selected result by asset id (or null to deselect). */
  selectAsset: (assetId: number | null) => void;
  /** Suggest asset codes for typeahead. */
  suggestCodes: (prefix: string) => Promise<AssetSuggestion[]>;
  /** Suggest asset names for typeahead. */
  suggestNames: (partial: string) => Promise<AssetSuggestion[]>;
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useAssetSearchStore = create<AssetSearchStoreState>()((set, get) => ({
  filters: { ...DEFAULT_FILTERS },
  results: [],
  selectedResultId: null,
  loading: false,
  error: null,

  runSearch: async () => {
    const { filters } = get();
    set({ loading: true, error: null });
    try {
      const results = await searchAssets(filters);
      set({ results });
    } catch (err) {
      set({
        results: [],
        error: toErrorMessage(err),
      });
    } finally {
      set({ loading: false });
    }
  },

  updateFilters: async (partial) => {
    const current = get().filters;
    const merged = { ...current, ...partial };
    set({ filters: merged, loading: true, error: null });
    try {
      const results = await searchAssets(merged);
      set({ results });
    } catch (err) {
      set({
        results: [],
        error: toErrorMessage(err),
      });
    } finally {
      set({ loading: false });
    }
  },

  clearFilters: async () => {
    const fresh = { ...DEFAULT_FILTERS };
    set({ filters: fresh, selectedResultId: null, loading: true, error: null });
    try {
      const results = await searchAssets(fresh);
      set({ results });
    } catch (err) {
      set({
        results: [],
        error: toErrorMessage(err),
      });
    } finally {
      set({ loading: false });
    }
  },

  selectAsset: (assetId) => {
    set({ selectedResultId: assetId });
  },

  suggestCodes: async (prefix) => {
    return suggestAssetCodes(prefix);
  },

  suggestNames: async (partial) => {
    return suggestAssetNames(partial);
  },
}));

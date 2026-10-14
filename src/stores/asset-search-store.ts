/**
 * asset-search-store.ts
 *
 * Zustand store for asset search query state and result management.
 * Maintains filter criteria, result list, tree expand state, and selected
 * result independently from the main asset-store (CRUD / hierarchy).
 */

import { create } from "zustand";

import {
  buildEquipmentForest,
  collectAncestorIdsToReveal,
  collectExpandableIds,
  pruneExpandedIds,
} from "@/components/assets/tree/equipment-tree-model";
import {
  searchAssetsPage,
  suggestAssetCodes,
  suggestAssetNames,
} from "@/services/asset-search-service";
import type { AssetSearchFilters, AssetSearchResult, AssetSuggestion } from "@shared/ipc-types";

// ── Default filter state ──────────────────────────────────────────────────────

/** Rows fetched per page (backend maximum: 200). */
export const ASSET_PAGE_SIZE = 100;

const DEFAULT_FILTERS: AssetSearchFilters = {
  query: null,
  class_codes: null,
  family_codes: null,
  status_codes: null,
  org_node_ids: null,
  include_decommissioned: false,
  limit: ASSET_PAGE_SIZE,
  registry_focus: null,
  criticality_codes: null,
  org_subtree: true,
};

function applyResultsToExpandState(
  results: AssetSearchResult[],
  previousExpanded: Set<number>,
  query: string | null | undefined,
): Set<number> {
  const forest = buildEquipmentForest(results);
  let next = pruneExpandedIds(previousExpanded, forest);
  const trimmed = query?.trim() ?? "";
  if (trimmed.length > 0) {
    for (const id of collectAncestorIdsToReveal(forest)) {
      next.add(id);
    }
  }
  return next;
}

// ── Store interface ───────────────────────────────────────────────────────────

interface AssetSearchStoreState {
  filters: AssetSearchFilters;
  results: AssetSearchResult[];
  /** Assets matching the filters (results holds the pages loaded so far). */
  total: number;
  selectedResultId: number | null;
  loading: boolean;
  loadingMore: boolean;
  /** Raw failure of the last load (rendered through ErrorState / describeAppError). */
  error: unknown;
  /** Expanded parent asset ids for the list tree (session until page refresh). */
  expandedIds: Set<number>;

  /** Execute search with current filters. */
  runSearch: () => Promise<void>;
  /** Load the next page with the same filters. */
  loadMore: () => Promise<void>;
  /** Merge partial filter updates and re-execute search. */
  updateFilters: (partial: Partial<AssetSearchFilters>) => Promise<void>;
  /** Reset all filters to defaults and re-execute search. */
  clearFilters: () => Promise<void>;
  /** Set the selected result by asset id (or null to deselect). */
  selectAsset: (assetId: number | null) => void;
  toggleExpanded: (assetId: number) => void;
  expandAll: () => void;
  collapseAll: () => void;
  /** Suggest asset codes for typeahead. */
  suggestCodes: (prefix: string) => Promise<AssetSuggestion[]>;
  /** Suggest asset names for typeahead. */
  suggestNames: (partial: string) => Promise<AssetSuggestion[]>;
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useAssetSearchStore = create<AssetSearchStoreState>()((set, get) => {
  /** Loads the first page for `filters`; later answers win over earlier ones. */
  let requestSeq = 0;
  const loadFirstPage = async (filters: AssetSearchFilters) => {
    const seq = ++requestSeq;
    set({ loading: true, error: null });
    try {
      const page = await searchAssetsPage({ ...filters, offset: 0, limit: ASSET_PAGE_SIZE });
      if (seq !== requestSeq) return;
      set({
        results: page.items,
        total: page.total,
        expandedIds: applyResultsToExpandState(page.items, get().expandedIds, filters.query),
      });
    } catch (err) {
      if (seq !== requestSeq) return;
      set({ results: [], total: 0, error: err });
    } finally {
      if (seq === requestSeq) set({ loading: false });
    }
  };

  return {
    filters: { ...DEFAULT_FILTERS },
    results: [],
    total: 0,
    selectedResultId: null,
    loading: false,
    loadingMore: false,
    error: null,
    expandedIds: new Set(),

    runSearch: async () => {
      await loadFirstPage(get().filters);
    },

    loadMore: async () => {
      const { filters, results, total, loadingMore } = get();
      if (loadingMore || results.length >= total) return;
      const seq = requestSeq;
      set({ loadingMore: true });
      try {
        const page = await searchAssetsPage({
          ...filters,
          offset: results.length,
          limit: ASSET_PAGE_SIZE,
        });
        if (seq !== requestSeq) return;
        const merged = [...get().results, ...page.items];
        set({
          results: merged,
          total: page.total,
          expandedIds: applyResultsToExpandState(merged, get().expandedIds, filters.query),
        });
      } catch (err) {
        if (seq === requestSeq) set({ error: err });
      } finally {
        set({ loadingMore: false });
      }
    },

    updateFilters: async (partial) => {
      const merged = { ...get().filters, ...partial };
      set({ filters: merged });
      await loadFirstPage(merged);
    },

    clearFilters: async () => {
      const fresh = { ...DEFAULT_FILTERS };
      set({ filters: fresh, selectedResultId: null });
      await loadFirstPage(fresh);
    },

    selectAsset: (assetId) => {
      set({ selectedResultId: assetId });
    },

    toggleExpanded: (assetId) => {
      const next = new Set(get().expandedIds);
      if (next.has(assetId)) next.delete(assetId);
      else next.add(assetId);
      set({ expandedIds: next });
    },

    expandAll: () => {
      const forest = buildEquipmentForest(get().results);
      set({ expandedIds: new Set(collectExpandableIds(forest)) });
    },

    collapseAll: () => {
      set({ expandedIds: new Set() });
    },

    suggestCodes: async (prefix) => {
      return suggestAssetCodes(prefix);
    },

    suggestNames: async (partial) => {
      return suggestAssetNames(partial);
    },
  };
});

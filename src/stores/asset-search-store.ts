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
  searchAssets,
  suggestAssetCodes,
  suggestAssetNames,
} from "@/services/asset-search-service";
import { toErrorMessage } from "@/utils/errors";
import type { AssetSearchFilters, AssetSearchResult, AssetSuggestion } from "@shared/ipc-types";

// ── Default filter state ──────────────────────────────────────────────────────

const DEFAULT_FILTERS: AssetSearchFilters = {
  query: null,
  class_codes: null,
  family_codes: null,
  status_codes: null,
  org_node_ids: null,
  include_decommissioned: false,
  limit: 100,
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
  selectedResultId: number | null;
  loading: boolean;
  error: string | null;
  /** Expanded parent asset ids for the list tree (session until page refresh). */
  expandedIds: Set<number>;

  /** Execute search with current filters. */
  runSearch: () => Promise<void>;
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

export const useAssetSearchStore = create<AssetSearchStoreState>()((set, get) => ({
  filters: { ...DEFAULT_FILTERS },
  results: [],
  selectedResultId: null,
  loading: false,
  error: null,
  expandedIds: new Set(),

  runSearch: async () => {
    const { filters, expandedIds } = get();
    set({ loading: true, error: null });
    try {
      const results = await searchAssets(filters);
      set({
        results,
        expandedIds: applyResultsToExpandState(results, expandedIds, filters.query),
      });
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
    const { expandedIds } = get();
    set({ filters: merged, loading: true, error: null });
    try {
      const results = await searchAssets(merged);
      set({
        results,
        expandedIds: applyResultsToExpandState(results, expandedIds, merged.query),
      });
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
    const { expandedIds } = get();
    set({ filters: fresh, selectedResultId: null, loading: true, error: null });
    try {
      const results = await searchAssets(fresh);
      set({
        results,
        expandedIds: applyResultsToExpandState(results, expandedIds, fresh.query),
      });
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
}));

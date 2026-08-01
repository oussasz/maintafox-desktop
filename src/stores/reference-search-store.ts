/**
 * reference-search-store.ts
 *
 * Zustand store for alias-aware reference value search.
 * Manages query state, locale, domain scope, ranked results, and error/loading flags
 * for the Lookup and Reference Data Manager search UI.
 *
 * Phase 2 – Sub-phase 03 – File 03 – Sprint S3.
 */

import { create } from "zustand";

import { searchReferenceValues } from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type { ReferenceSearchHit } from "@shared/ipc-types";

// ── Store interface ───────────────────────────────────────────────────────────

interface ReferenceSearchStoreState {
  query: string;
  locale: string;
  domain: string;
  results: ReferenceSearchHit[];
  loading: boolean;
  error: string | null;

  /** Execute search with current query/locale/domain. */
  runSearch: () => Promise<void>;
  /** Update query text and re-execute search. */
  setQuery: (query: string) => Promise<void>;
  /** Change locale and re-execute search (if query is non-empty). */
  setLocale: (locale: string) => Promise<void>;
  /** Change target domain and re-execute search (if query is non-empty). */
  setDomain: (domain: string) => Promise<void>;
  /** Reset all fields to defaults. */
  reset: () => void;
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useReferenceSearchStore = create<ReferenceSearchStoreState>()((set, get) => ({
  query: "",
  locale: "fr",
  domain: "",
  results: [],
  loading: false,
  error: null,

  runSearch: async () => {
    const { query, locale, domain } = get();
    if (!query.trim() || !domain) {
      set({ results: [], error: null });
      return;
    }
    set({ loading: true, error: null });
    try {
      const results = await searchReferenceValues(domain, query, locale);
      set({ results });
    } catch (err) {
      set({ results: [], error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  setQuery: async (query: string) => {
    set({ query });
    await get().runSearch();
  },

  setLocale: async (locale: string) => {
    set({ locale });
    if (get().query.trim()) {
      await get().runSearch();
    }
  },

  setDomain: async (domain: string) => {
    set({ domain });
    if (get().query.trim()) {
      await get().runSearch();
    }
  },

  reset: () => {
    set({
      query: "",
      locale: "fr",
      domain: "",
      results: [],
      loading: false,
      error: null,
    });
  },
}));

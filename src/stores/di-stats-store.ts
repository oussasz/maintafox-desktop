/**
 * di-stats-store.ts
 *
 * Zustand store for DI dashboard statistics.
 * Phase 2 – Sub-phase 04 – File 04 – Sprint S4.
 */

import { create } from "zustand";

import { getDiStats } from "@/services/di-stats-service";
import { toErrorMessage } from "@/utils/errors";
import type { DiStatsFilter, DiStatsPayload } from "@shared/ipc-types";

interface DiStatsStoreState {
  stats: DiStatsPayload | null;
  filter: DiStatsFilter;
  loading: boolean;
  error: string | null;

  setFilter: (patch: Partial<DiStatsFilter>) => void;
  loadStats: () => Promise<void>;
}

export const useDiStatsStore = create<DiStatsStoreState>()((set, get) => ({
  stats: null,
  filter: {},
  loading: false,
  error: null,

  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },

  loadStats: async () => {
    set({ loading: true, error: null });
    try {
      const stats = await getDiStats(get().filter);
      set({ stats });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },
}));

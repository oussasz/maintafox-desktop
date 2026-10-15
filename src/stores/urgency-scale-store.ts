/**
 * Shared DI / OT urgency scale (`WORK.URGENCY`) for forms, lists, filters and badges.
 * Loaded once from the backend; refreshed after the plant edits a level.
 */

import { create } from "zustand";

import { listUrgencyLevels } from "@/services/urgency-scale-service";
import { toErrorMessage } from "@/utils/errors";
import type { UrgencyLevel } from "@shared/ipc-types";

interface State {
  levels: UrgencyLevel[];
  loaded: boolean;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  /** Loads once; later calls are no-ops while loaded or loading. */
  ensureLoaded: () => Promise<void>;
}

export const useUrgencyScaleStore = create<State>((set, get) => ({
  levels: [],
  loaded: false,
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const levels = await listUrgencyLevels();
      set({ levels: [...levels].sort((a, b) => a.rank - b.rank), loaded: true, loading: false });
    } catch (err) {
      set({ loading: false, error: toErrorMessage(err) });
    }
  },
  ensureLoaded: async () => {
    const { loaded, loading, load } = get();
    if (loaded || loading) return;
    await load();
  },
}));

export function refreshUrgencyScale(): Promise<void> {
  return useUrgencyScaleStore.getState().load();
}

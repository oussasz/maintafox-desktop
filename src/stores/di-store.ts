/**
 * di-store.ts
 *
 * Zustand store for intervention requests (DI).
 * Manages the paginated list, selected DI detail, filters, and CRUD operations.
 * Phase 2 – Sub-phase 04 – File 01 – Sprint S3.
 */

import { create } from "zustand";

import { listDis, getDi, createDi, updateDiDraft } from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  InterventionRequest,
  DiListFilter,
  DiTransitionRow,
  DiSummaryRow,
  DiCreateInput,
  DiDraftUpdateInput,
} from "@shared/ipc-types";

export interface DiDetailPayload {
  di: InterventionRequest;
  transitions: DiTransitionRow[];
  similar: DiSummaryRow[];
}

const DEFAULT_FILTER: DiListFilter = { limit: 50, offset: 0 };

interface DiStoreState {
  // List
  items: InterventionRequest[];
  total: number;
  // Detail
  activeDi: DiDetailPayload | null;
  // Filter
  filter: DiListFilter;
  // Flags
  loading: boolean;
  saving: boolean;
  error: string | null;

  setFilter: (patch: Partial<DiListFilter>) => void;
  loadDis: () => Promise<void>;
  openDi: (id: number) => Promise<void>;
  submitNewDi: (input: DiCreateInput) => Promise<InterventionRequest>;
  updateDraft: (input: DiDraftUpdateInput) => Promise<void>;
}

export const useDiStore = create<DiStoreState>()((set, get) => ({
  items: [],
  total: 0,
  activeDi: null,
  filter: { ...DEFAULT_FILTER },
  loading: false,
  saving: false,
  error: null,

  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },

  loadDis: async () => {
    set({ loading: true, error: null });
    try {
      const page = await listDis(get().filter);
      set({ items: page.items, total: page.total });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  openDi: async (id) => {
    set({ loading: true, error: null });
    try {
      const detail = await getDi(id);
      set({
        activeDi: {
          di: detail.di,
          transitions: detail.transitions,
          similar: detail.similar,
        },
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  submitNewDi: async (input) => {
    set({ saving: true, error: null });
    try {
      const di = await createDi(input);
      // Reload list to include the new DI
      const page = await listDis(get().filter);
      set({ items: page.items, total: page.total });
      return di;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updateDraft: async (input) => {
    set({ saving: true, error: null });
    try {
      const di = await updateDiDraft(input);
      // Refresh activeDi with updated record
      const detail = await getDi(di.id);
      set({
        activeDi: {
          di: detail.di,
          transitions: detail.transitions,
          similar: detail.similar,
        },
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));

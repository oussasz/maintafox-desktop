/**
 * di-store.ts
 *
 * Zustand store for intervention requests (DI).
 * Manages the paginated list, selected DI detail, filters, and CRUD operations.
 * Phase 2 – Sub-phase 04 – File 01 – Sprint S3.
 */

import { create } from "zustand";

import { createLatestRequestGuard } from "@/lib/latest-request";
import {
  createDi,
  getDi,
  getDiQueueCounts,
  listDis,
  resubmitDi,
  triageSubmittedDi as triageSubmittedDiCommand,
  updateDiDraft,
} from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  AssetSearchResult,
  DiCreateInput,
  DiDraftUpdateInput,
  DiListFilter,
  DiQueueCounts,
  DiResubmitInput,
  DiTriageSubmittedInput,
  DiSummaryRow,
  DiTransitionRow,
  InterventionRequest,
} from "@shared/ipc-types";

export interface DiDetailPayload {
  di: InterventionRequest;
  transitions: DiTransitionRow[];
  similar: DiSummaryRow[];
}

/** Page size of the request list; matches the backend maximum (`di/queries.rs` clamps to 200). */
export const DI_LIST_PAGE_SIZE = 200;

const DEFAULT_FILTER: DiListFilter = { limit: DI_LIST_PAGE_SIZE, offset: 0 };

const requests = createLatestRequestGuard();

interface DiStoreState {
  // List
  items: InterventionRequest[];
  total: number;
  // Detail
  activeDi: DiDetailPayload | null;
  // Create / edit form
  showCreateForm: boolean;
  editingDi: InterventionRequest | null;
  /** Prefill equipment when opening create from asset context. */
  createPrefillAsset: AssetSearchResult | null;
  createPrefillEquipmentId: number | null;
  // Filter
  filter: DiListFilter;
  // Flags
  loading: boolean;
  loadingMore: boolean;
  saving: boolean;
  error: string | null;
  /** Raw failure of the last list load (for `ErrorState`); null when the list loaded. */
  listError: unknown;
  /** Size of every work queue; null until loaded. */
  queueCounts: DiQueueCounts | null;
  /** Raw failure of the last queue count load. */
  queueCountsError: unknown;

  setFilter: (patch: Partial<DiListFilter>) => void;
  /** Reloads the list from the first page with the current filter. */
  loadDis: () => Promise<void>;
  /** Appends the next page (offset = items already loaded). */
  loadMoreDis: () => Promise<void>;
  openDi: (id: number) => Promise<void>;
  closeDi: () => void;
  openCreateForm: (di?: InterventionRequest, prefillAsset?: AssetSearchResult | null) => void;
  closeCreateForm: () => void;
  submitNewDi: (input: DiCreateInput) => Promise<InterventionRequest>;
  updateDraft: (input: DiDraftUpdateInput) => Promise<void>;
  triageSubmittedDi: (input: DiTriageSubmittedInput) => Promise<InterventionRequest>;
  /** The requester answers the reviewer and sends the request back to review. */
  resubmit: (input: DiResubmitInput) => Promise<InterventionRequest>;
  loadQueueCounts: () => Promise<void>;
}

export const useDiStore = create<DiStoreState>()((set, get) => ({
  items: [],
  total: 0,
  activeDi: null,
  showCreateForm: false,
  editingDi: null,
  createPrefillAsset: null,
  createPrefillEquipmentId: null,
  filter: { ...DEFAULT_FILTER },
  loading: false,
  loadingMore: false,
  saving: false,
  error: null,
  listError: null,
  queueCounts: null,
  queueCountsError: null,

  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },

  closeDi: () => {
    set({ activeDi: null });
  },

  openCreateForm: (di, prefillAsset) => {
    set({
      showCreateForm: true,
      editingDi: di ?? null,
      createPrefillAsset: di ? null : (prefillAsset ?? null),
      createPrefillEquipmentId: di ? null : (prefillAsset?.id ?? null),
    });
  },

  closeCreateForm: () => {
    set({
      showCreateForm: false,
      editingDi: null,
      createPrefillAsset: null,
      createPrefillEquipmentId: null,
    });
  },

  loadDis: async () => {
    // Filters change faster than the list loads: only the latest list may land.
    const isCurrent = requests.begin("loadDis");
    set({ loading: true, error: null, listError: null });
    try {
      const page = await listDis({ ...get().filter, offset: 0 });
      if (!isCurrent()) return;
      set({ items: page.items, total: page.total, loading: false });
    } catch (err) {
      if (!isCurrent()) return;
      set({ error: toErrorMessage(err), listError: err, loading: false });
    }
  },

  loadMoreDis: async () => {
    const { items, total, loadingMore, loading } = get();
    if (loadingMore || loading || items.length >= total) return;
    const isCurrent = requests.begin("loadDis");
    set({ loadingMore: true });
    try {
      const page = await listDis({ ...get().filter, offset: items.length });
      if (!isCurrent()) return;
      const known = new Set(items.map((d) => d.id));
      set({
        items: [...items, ...page.items.filter((d) => !known.has(d.id))],
        total: page.total,
        loadingMore: false,
      });
    } catch {
      // The global IPC toast reports the failure; the loaded pages stay usable.
      if (isCurrent()) set({ loadingMore: false });
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
      void get().loadQueueCounts();
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
      // Refresh the detail and the list with the updated record
      const [detail, page] = await Promise.all([getDi(di.id), listDis(get().filter)]);
      set({
        activeDi: {
          di: detail.di,
          transitions: detail.transitions,
          similar: detail.similar,
        },
        items: page.items,
        total: page.total,
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  loadQueueCounts: async () => {
    const isCurrent = requests.begin("loadQueueCounts");
    try {
      const counts = await getDiQueueCounts();
      if (isCurrent()) set({ queueCounts: counts, queueCountsError: null });
    } catch (err) {
      if (isCurrent()) set({ queueCountsError: err });
    }
  },

  resubmit: async (input) => {
    set({ saving: true, error: null });
    try {
      const di = await resubmitDi(input);
      const [page, detail] = await Promise.all([listDis(get().filter), getDi(di.id)]);
      set({
        items: page.items,
        total: page.total,
        activeDi: { di: detail.di, transitions: detail.transitions, similar: detail.similar },
      });
      void get().loadQueueCounts();
      return di;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  triageSubmittedDi: async (input) => {
    set({ saving: true, error: null });
    try {
      const di = await triageSubmittedDiCommand(input);
      const page = await listDis(get().filter);
      set({ items: page.items, total: page.total });
      void get().loadQueueCounts();
      if (get().activeDi?.di.id === di.id) {
        const detail = await getDi(di.id);
        set({
          activeDi: {
            di: detail.di,
            transitions: detail.transitions,
            similar: detail.similar,
          },
        });
      }
      return di;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));

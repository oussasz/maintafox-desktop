/**
 * wo-store.ts
 *
 * Zustand store for work orders (OT).
 * Manages the paginated list, selected WO detail, filters, and CRUD operations.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S3 (updated from S4 scaffold).
 */

import { create } from "zustand";

import { createLatestRequestGuard } from "@/lib/latest-request";
import { closeWo } from "@/services/wo-closeout-service";
import {
  CompletionBlockedError,
  assignWo,
  cancelWo,
  completeWoMechanically,
  createWo,
  getWo,
  listWos,
  pauseWo,
  planWo,
  resumeWo,
  startWo,
  updateWoDraft,
} from "@/services/wo-service";
import { isStepUpRequiredError, toErrorMessage } from "@/utils/errors";
import type {
  AssetSearchResult,
  WoAssignInput,
  WoCancelInput,
  WoCloseInput,
  WoCreateInput,
  WoDraftUpdateInput,
  WoListFilter,
  WoMechCompleteInput,
  WoMechCompleteResponse,
  WoPauseInput,
  WoPlanInput,
  WoPreflightError,
  WoResumeInput,
  WoStartInput,
  WoTransitionRow,
  WorkOrder,
} from "@shared/ipc-types";

export interface WoDetailPayload {
  wo: WorkOrder;
  transitions: WoTransitionRow[];
}

/** Page size of the work-order list; matches the backend maximum (`wo/queries.rs` clamps to 200). */
export const WO_LIST_PAGE_SIZE = 200;

const DEFAULT_FILTER: WoListFilter = { limit: WO_LIST_PAGE_SIZE, offset: 0 };

const requests = createLatestRequestGuard();

interface WoStoreState {
  // List
  items: WorkOrder[];
  total: number;
  /** Work orders outside the user’s sites (not listed, shown as a count, whatever the filters). */
  hiddenOutsideScope: number;
  /** Work orders without an org node (plant-wide users only), whatever the filters. */
  hiddenWithoutSite: number;
  // Detail
  activeWo: WoDetailPayload | null;
  /** True while openWo is fetching (dialog may show loading shell). */
  detailLoading: boolean;
  openingWoId: number | null;
  // Create / edit form
  showCreateForm: boolean;
  editingWo: WorkOrder | null;
  /** Prefill equipment when opening create from asset context. */
  createPrefillAsset: AssetSearchResult | null;
  createPrefillEquipmentId: number | null;
  // Completion dialog
  showCompletionDialog: boolean;
  completionErrors: WoPreflightError[];
  // Filter
  filter: WoListFilter;
  // Flags
  loading: boolean;
  loadingMore: boolean;
  saving: boolean;
  error: string | null;
  /** Raw failure of the last list load (for `ErrorState`); null when the list loaded. */
  listError: unknown;

  setFilter: (patch: Partial<WoListFilter>) => void;
  /** Reloads the list from the first page with the current filter. */
  loadWos: () => Promise<void>;
  /** Appends the next page (offset = items already loaded). */
  loadMoreWos: () => Promise<void>;
  openWo: (id: number) => Promise<void>;
  closeActiveWo: () => void;
  openCreateForm: (wo?: WorkOrder, prefillAsset?: AssetSearchResult | null) => void;
  closeCreateForm: () => void;
  submitNewWo: (input: WoCreateInput) => Promise<WorkOrder>;
  updateDraft: (input: WoDraftUpdateInput) => Promise<void>;
  cancel: (input: WoCancelInput) => Promise<void>;
  // Planning & scheduling
  planWorkOrder: (input: WoPlanInput) => Promise<void>;
  assignWorkOrder: (input: WoAssignInput) => Promise<void>;
  // Execution lifecycle
  startWorkOrder: (input: WoStartInput) => Promise<void>;
  pauseWorkOrder: (input: WoPauseInput) => Promise<void>;
  resumeWorkOrder: (input: WoResumeInput) => Promise<void>;
  openCompletionDialog: () => void;
  closeCompletionDialog: () => void;
  completeWorkOrder: (input: WoMechCompleteInput) => Promise<WoMechCompleteResponse>;
  closeWorkOrder: (input: WoCloseInput) => Promise<void>;
  cancelWorkOrder: (input: WoCancelInput) => Promise<void>;
  // Util
  refreshActiveWo: () => Promise<void>;
}

export const useWoStore = create<WoStoreState>()((set, get) => ({
  items: [],
  total: 0,
  hiddenOutsideScope: 0,
  hiddenWithoutSite: 0,
  activeWo: null,
  detailLoading: false,
  openingWoId: null,
  showCreateForm: false,
  editingWo: null,
  createPrefillAsset: null,
  createPrefillEquipmentId: null,
  showCompletionDialog: false,
  completionErrors: [],
  filter: { ...DEFAULT_FILTER },
  loading: false,
  loadingMore: false,
  saving: false,
  error: null,
  listError: null,

  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },

  closeActiveWo: () => {
    // A detail still loading must not reopen the dialog that was just closed.
    requests.invalidate("openWo");
    set({ activeWo: null, detailLoading: false, openingWoId: null });
  },

  openCreateForm: (wo, prefillAsset) => {
    set({
      showCreateForm: true,
      editingWo: wo ?? null,
      createPrefillAsset: wo ? null : (prefillAsset ?? null),
      createPrefillEquipmentId: wo ? null : (prefillAsset?.id ?? null),
    });
  },

  closeCreateForm: () => {
    set({
      showCreateForm: false,
      editingWo: null,
      createPrefillAsset: null,
      createPrefillEquipmentId: null,
    });
  },

  loadWos: async () => {
    // Filters change faster than the list loads: only the latest list may land.
    const isCurrent = requests.begin("loadWos");
    set({ loading: true, error: null, listError: null });
    try {
      const page = await listWos({ ...get().filter, offset: 0 });
      if (!isCurrent()) return;
      set({
        items: page.items,
        total: page.total,
        hiddenOutsideScope: page.hidden_outside_scope,
        hiddenWithoutSite: page.hidden_without_site,
        loading: false,
      });
    } catch (err) {
      if (!isCurrent()) return;
      set({ error: toErrorMessage(err), listError: err, loading: false });
    }
  },

  loadMoreWos: async () => {
    const { items, total, loadingMore, loading } = get();
    if (loadingMore || loading || items.length >= total) return;
    const isCurrent = requests.begin("loadWos");
    set({ loadingMore: true });
    try {
      const page = await listWos({ ...get().filter, offset: items.length });
      if (!isCurrent()) return;
      const known = new Set(items.map((w) => w.id));
      set({
        items: [...items, ...page.items.filter((w) => !known.has(w.id))],
        total: page.total,
        hiddenOutsideScope: page.hidden_outside_scope,
        hiddenWithoutSite: page.hidden_without_site,
        loadingMore: false,
      });
    } catch {
      // The global IPC toast reports the failure; the loaded pages stay usable.
      if (isCurrent()) set({ loadingMore: false });
    }
  },

  openWo: async (id) => {
    const isCurrent = requests.begin("openWo");
    const current = get().activeWo;
    const keepCurrent = current?.wo.id === id;
    set({
      detailLoading: true,
      openingWoId: id,
      error: null,
      ...(keepCurrent ? {} : { activeWo: null }),
    });
    try {
      const resp = await getWo(id);
      if (!isCurrent()) return;
      set({
        activeWo: { wo: resp.wo, transitions: resp.transitions },
        detailLoading: false,
        openingWoId: null,
      });
    } catch (err) {
      if (!isCurrent()) return;
      set({
        error: toErrorMessage(err),
        detailLoading: false,
        openingWoId: null,
        ...(keepCurrent ? {} : { activeWo: null }),
      });
    }
  },

  submitNewWo: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await createWo(input);
      // Refresh list after create
      void get().loadWos();
      set({
        showCreateForm: false,
        editingWo: null,
        createPrefillAsset: null,
        createPrefillEquipmentId: null,
      });
      return wo;
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
      const updated = await updateWoDraft(input);
      set({
        activeWo: { wo: updated, transitions: get().activeWo?.transitions ?? [] },
        showCreateForm: false,
        editingWo: null,
      });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  cancel: async (input) => {
    set({ saving: true, error: null });
    try {
      await cancelWo(input);
      // Re-fetch to get updated transitions log
      void get().refreshActiveWo();
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  // ── Planning & scheduling ───────────────────────────────────────────

  planWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await planWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  assignWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await assignWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  // ── Execution lifecycle ─────────────────────────────────────────────

  startWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await startWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  pauseWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await pauseWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  resumeWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await resumeWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  openCompletionDialog: () => {
    set({ showCompletionDialog: true, completionErrors: [] });
  },

  closeCompletionDialog: () => {
    set({ showCompletionDialog: false, completionErrors: [] });
  },

  completeWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const resp = await completeWoMechanically(input);
      set({
        activeWo: { wo: resp.wo, transitions: get().activeWo?.transitions ?? [] },
        completionErrors: resp.errors,
      });
      if (resp.errors.length === 0) {
        set({ showCompletionDialog: false });
      }
      void get().loadWos();
      return resp;
    } catch (err) {
      if (err instanceof CompletionBlockedError) {
        set({ completionErrors: err.errors, showCompletionDialog: true });
        return {
          wo: get().activeWo?.wo,
          errors: err.errors,
        } as WoMechCompleteResponse;
      }
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  closeWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await closeWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  cancelWorkOrder: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await cancelWo(input);
      set({ activeWo: { wo, transitions: get().activeWo?.transitions ?? [] } });
      void get().loadWos();
    } catch (err) {
      // A password request is not a failure to keep on screen (the dialog asks and retries).
      if (!isStepUpRequiredError(err)) set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  // ── Util ────────────────────────────────────────────────────────────

  refreshActiveWo: async () => {
    const active = get().activeWo;
    if (!active) return;
    try {
      const resp = await getWo(active.wo.id);
      set({ activeWo: { wo: resp.wo, transitions: resp.transitions } });
    } catch {
      // Silently ignore — the WO may have been deleted
    }
  },
}));

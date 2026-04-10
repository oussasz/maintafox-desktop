/**
 * wo-store.ts
 *
 * Zustand store for work orders (OT).
 * Manages the paginated list, selected WO detail, filters, and CRUD operations.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S3 (updated from S4 scaffold).
 */

import { create } from "zustand";

import {
  assignWo,
  cancelWo,
  closeWo,
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
import { toErrorMessage } from "@/utils/errors";
import type {
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

const DEFAULT_FILTER: WoListFilter = { limit: 50, offset: 0 };

interface WoStoreState {
  // List
  items: WorkOrder[];
  total: number;
  // Detail
  activeWo: WoDetailPayload | null;
  // Create / edit form
  showCreateForm: boolean;
  editingWo: WorkOrder | null;
  // Completion dialog
  showCompletionDialog: boolean;
  completionErrors: WoPreflightError[];
  // Filter
  filter: WoListFilter;
  // Flags
  loading: boolean;
  saving: boolean;
  error: string | null;

  setFilter: (patch: Partial<WoListFilter>) => void;
  loadWos: () => Promise<void>;
  openWo: (id: number) => Promise<void>;
  closeActiveWo: () => void;
  openCreateForm: (wo?: WorkOrder) => void;
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
  activeWo: null,
  showCreateForm: false,
  editingWo: null,
  showCompletionDialog: false,
  completionErrors: [],
  filter: { ...DEFAULT_FILTER },
  loading: false,
  saving: false,
  error: null,

  setFilter: (patch) => {
    set((s) => ({ filter: { ...s.filter, ...patch } }));
  },

  closeActiveWo: () => {
    set({ activeWo: null });
  },

  openCreateForm: (wo) => {
    set({ showCreateForm: true, editingWo: wo ?? null });
  },

  closeCreateForm: () => {
    set({ showCreateForm: false, editingWo: null });
  },

  loadWos: async () => {
    set({ loading: true, error: null });
    try {
      const page = await listWos(get().filter);
      set({ items: page.items, total: page.total });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  openWo: async (id) => {
    set({ loading: true, error: null });
    try {
      const resp = await getWo(id);
      set({ activeWo: { wo: resp.wo, transitions: resp.transitions } });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  submitNewWo: async (input) => {
    set({ saving: true, error: null });
    try {
      const wo = await createWo(input);
      // Refresh list after create
      void get().loadWos();
      set({ showCreateForm: false, editingWo: null });
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
      set({ error: toErrorMessage(err) });
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

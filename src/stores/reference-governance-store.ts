/**
 * reference-governance-store.ts
 *
 * UI state for publish-readiness validation, impact preview, governed
 * publish workflow, and value CRUD for reference sets.
 *
 * Phase 2 – Sub-phase 03 – Files 02–04 – Sprint S4.
 */

import { create } from "zustand";

import {
  computeRefPublishReadiness,
  createReferenceValue,
  deactivateReferenceValue,
  governedPublishReferenceSet,
  listReferenceValues,
  previewRefPublishImpact,
  updateReferenceValue,
} from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  CreateReferenceValuePayload,
  ReferenceImpactSummary,
  ReferencePublishReadiness,
  ReferenceValue,
  UpdateReferenceValuePayload,
} from "@shared/ipc-types";

// ── Store interface ───────────────────────────────────────────────────────────

interface ReferenceGovernanceStoreState {
  /** Values for the currently selected set */
  values: ReferenceValue[];
  valuesLoading: boolean;
  /** ID of the value currently in inline-edit mode (null = none) */
  editingValueId: number | null;
  /** Transient new-value row being created (null = none) */
  newValueDraft: Partial<CreateReferenceValuePayload> | null;
  savingValue: boolean;

  readiness: ReferencePublishReadiness | null;
  readinessLoading: boolean;
  impactSummary: ReferenceImpactSummary | null;
  impactLoading: boolean;
  error: string | null;

  loadValues: (setId: number) => Promise<void>;
  saveValue: (valueId: number, payload: UpdateReferenceValuePayload) => Promise<void>;
  addValue: (payload: CreateReferenceValuePayload) => Promise<void>;
  removeValue: (valueId: number) => Promise<void>;
  setEditingValueId: (id: number | null) => void;
  setNewValueDraft: (draft: Partial<CreateReferenceValuePayload> | null) => void;

  loadReadiness: (setId: number) => Promise<void>;
  loadImpact: (setId: number) => Promise<void>;
  publish: (setId: number) => Promise<void>;
  clearError: () => void;
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useReferenceGovernanceStore = create<ReferenceGovernanceStoreState>()((set, get) => ({
  values: [],
  valuesLoading: false,
  editingValueId: null,
  newValueDraft: null,
  savingValue: false,

  readiness: null,
  readinessLoading: false,
  impactSummary: null,
  impactLoading: false,
  error: null,

  loadValues: async (setId) => {
    set({ valuesLoading: true, error: null, editingValueId: null, newValueDraft: null });
    try {
      const values = await listReferenceValues(setId);
      set({ values });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ valuesLoading: false });
    }
  },

  saveValue: async (valueId, payload) => {
    set({ savingValue: true, error: null });
    try {
      const updated = await updateReferenceValue(valueId, payload);
      set({
        values: get().values.map((v) => (v.id === valueId ? updated : v)),
        editingValueId: null,
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  addValue: async (payload) => {
    set({ savingValue: true, error: null });
    try {
      const created = await createReferenceValue(payload);
      set({ values: [created, ...get().values], newValueDraft: null });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  removeValue: async (valueId) => {
    set({ savingValue: true, error: null });
    try {
      await deactivateReferenceValue(valueId);
      set({ values: get().values.filter((v) => v.id !== valueId) });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  setEditingValueId: (id) => set({ editingValueId: id, newValueDraft: null }),
  setNewValueDraft: (draft) => set({ newValueDraft: draft, editingValueId: null }),

  loadReadiness: async (setId) => {
    set({ readinessLoading: true, error: null });
    try {
      const readiness = await computeRefPublishReadiness(setId);
      set({ readiness });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ readinessLoading: false });
    }
  },

  loadImpact: async (setId) => {
    set({ impactLoading: true, error: null });
    try {
      const impactSummary = await previewRefPublishImpact(setId);
      set({ impactSummary });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ impactLoading: false });
    }
  },

  publish: async (setId) => {
    set({ readinessLoading: true, error: null });
    try {
      const result = await governedPublishReferenceSet(setId);
      set({ readiness: result.readiness });
    } catch (err) {
      set({ error: toErrorMessage(err) });
      // Reload readiness so UI reflects current blockers
      try {
        const readiness = await computeRefPublishReadiness(setId);
        set({ readiness });
      } catch {
        // Readiness refresh failed — keep original error
      }
    } finally {
      set({ readinessLoading: false });
    }
  },

  clearError: () => set({ error: null }),
}));

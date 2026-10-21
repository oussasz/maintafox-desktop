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
  reactivateReferenceValue,
  updateReferenceValue,
  validateReferenceSet,
} from "@/services/reference-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import type {
  CreateReferenceValuePayload,
  ReferenceImpactSummary,
  ReferencePublishReadiness,
  ReferencePublishResult,
  ReferenceValue,
  UpdateReferenceValuePayload,
} from "@shared/ipc-types";

// ── Store interface ───────────────────────────────────────────────────────────

interface ReferenceGovernanceStoreState {
  /** Values for the currently selected set */
  values: ReferenceValue[];
  valuesLoading: boolean;
  /** The values of the selected set could not be loaded (shown in place of the table). */
  valuesError: string | null;
  /** ID of the value currently in inline-edit mode (null = none) */
  editingValueId: number | null;
  /** Transient new-value row being created (null = none) */
  newValueDraft: Partial<CreateReferenceValuePayload> | null;
  savingValue: boolean;

  readiness: ReferencePublishReadiness | null;
  readinessLoading: boolean;
  /**
   * The readiness check of `setId` could not be read. Kept apart from `error` (shared by every
   * reference action) so the readiness panel never shows an unrelated failure.
   */
  readinessError: { setId: number; error: unknown } | null;
  impactSummary: ReferenceImpactSummary | null;
  impactLoading: boolean;
  error: string | null;

  loadValues: (setId: number) => Promise<void>;
  saveValue: (valueId: number, payload: UpdateReferenceValuePayload) => Promise<void>;
  addValue: (payload: CreateReferenceValuePayload) => Promise<void>;
  /** Soft-deactivate; keeps the row in the list as inactive. */
  removeValue: (valueId: number) => Promise<void>;
  /** Toggle active flag (deactivate / reactivate); keeps row in place. */
  setValueActive: (valueId: number, active: boolean) => Promise<void>;
  setEditingValueId: (id: number | null) => void;
  setNewValueDraft: (draft: Partial<CreateReferenceValuePayload> | null) => void;

  loadReadiness: (setId: number) => Promise<void>;
  loadImpact: (setId: number) => Promise<void>;
  /**
   * Governed publish of a candidate set: validates a draft first (validation engine + report),
   * then publishes through the readiness gates. Rejects on failure (after refreshing readiness so
   * the blockers show) so a confirmation dialog can stay open.
   */
  validateAndPublish: (setId: number, setStatus: string) => Promise<ReferencePublishResult>;
  clearError: () => void;
}

function replaceValue(values: ReferenceValue[], updated: ReferenceValue): ReferenceValue[] {
  return values.map((v) => (v.id === updated.id ? updated : v));
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useReferenceGovernanceStore = create<ReferenceGovernanceStoreState>()((set, get) => ({
  values: [],
  valuesLoading: false,
  valuesError: null,
  editingValueId: null,
  newValueDraft: null,
  savingValue: false,

  readiness: null,
  readinessLoading: false,
  readinessError: null,
  impactSummary: null,
  impactLoading: false,
  error: null,

  loadValues: async (setId) => {
    set({
      valuesLoading: true,
      valuesError: null,
      error: null,
      editingValueId: null,
      newValueDraft: null,
    });
    try {
      const values = await listReferenceValues(setId);
      set({ values });
    } catch (err) {
      const text = referenceErrorText(err);
      set({ error: text, valuesError: text });
    } finally {
      set({ valuesLoading: false });
    }
  },

  saveValue: async (valueId, payload) => {
    set({ savingValue: true, error: null });
    try {
      const updated = await updateReferenceValue(valueId, payload);
      set({
        values: replaceValue(get().values, updated),
        editingValueId: null,
      });
    } catch (err) {
      set({ error: referenceErrorText(err) });
      throw err;
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
      set({ error: referenceErrorText(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  removeValue: async (valueId) => {
    set({ savingValue: true, error: null });
    try {
      const updated = await deactivateReferenceValue(valueId);
      set({ values: replaceValue(get().values, updated) });
    } catch (err) {
      set({ error: referenceErrorText(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  setValueActive: async (valueId, active) => {
    set({ savingValue: true, error: null });
    try {
      const updated = active
        ? await reactivateReferenceValue(valueId)
        : await deactivateReferenceValue(valueId);
      set({ values: replaceValue(get().values, updated) });
    } catch (err) {
      set({ error: referenceErrorText(err) });
    } finally {
      set({ savingValue: false });
    }
  },

  setEditingValueId: (id) => set({ editingValueId: id, newValueDraft: null }),
  setNewValueDraft: (draft) => set({ newValueDraft: draft, editingValueId: null }),

  loadReadiness: async (setId) => {
    set({ readinessLoading: true, error: null, readinessError: null });
    try {
      const readiness = await computeRefPublishReadiness(setId);
      set({ readiness });
    } catch (err) {
      set({ error: referenceErrorText(err), readinessError: { setId, error: err } });
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
      set({ error: referenceErrorText(err) });
    } finally {
      set({ impactLoading: false });
    }
  },

  validateAndPublish: async (setId, setStatus) => {
    set({ readinessLoading: true, error: null });
    try {
      if (setStatus === "draft") {
        await validateReferenceSet(setId);
      }
      const result = await governedPublishReferenceSet(setId);
      set({ readiness: result.readiness });
      return result;
    } catch (err) {
      set({ error: referenceErrorText(err) });
      // Reload readiness so the panel shows the current blockers.
      try {
        const readiness = await computeRefPublishReadiness(setId);
        set({ readiness });
      } catch {
        // Readiness refresh failed — keep the publish error.
      }
      throw err;
    } finally {
      set({ readinessLoading: false });
    }
  },

  clearError: () => set({ error: null }),
}));

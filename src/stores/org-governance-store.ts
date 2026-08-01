/**
 * org-governance-store.ts
 *
 * UI state for publish-readiness validation and the org audit timeline (SP01-F04).
 */

import { create } from "zustand";

import {
  listOrgChangeEvents,
  publishOrgModel,
  reconcileOrgDraftLineage,
  validateOrgModelForPublish,
} from "@/services/org-governance-service";
import { formatOrgIpcError } from "@/utils/errors";
import type { OrgChangeEvent, OrgPublishValidationResult } from "@shared/ipc-types";

interface OrgGovernanceStoreState {
  publishValidation: OrgPublishValidationResult | null;
  validationLoading: boolean;
  reconcileLoading: boolean;
  auditEvents: OrgChangeEvent[];
  auditLoading: boolean;
  error: string | null;

  loadPublishValidation: (modelId: number) => Promise<void>;
  reconcileDraftLineage: (draftModelId: number) => Promise<void>;
  publishModel: (modelId: number) => Promise<void>;
  loadAuditEvents: (limit?: number, entityKind?: string, entityId?: number) => Promise<void>;
  clearError: () => void;
}

export const useOrgGovernanceStore = create<OrgGovernanceStoreState>()((set, get) => ({
  publishValidation: null,
  validationLoading: false,
  reconcileLoading: false,
  auditEvents: [],
  auditLoading: false,
  error: null,

  loadPublishValidation: async (modelId) => {
    set({ validationLoading: true, error: null });
    try {
      const publishValidation = await validateOrgModelForPublish(modelId);
      set({ publishValidation });
    } catch (err) {
      set({ error: formatOrgIpcError(err) });
    } finally {
      set({ validationLoading: false });
    }
  },

  reconcileDraftLineage: async (draftModelId) => {
    set({ reconcileLoading: true, error: null });
    try {
      await reconcileOrgDraftLineage(draftModelId);
      const publishValidation = await validateOrgModelForPublish(draftModelId);
      set({ publishValidation });
    } catch (err) {
      set({ error: formatOrgIpcError(err) });
      // Keep publish disabled until validation succeeds again.
      const current = get().publishValidation;
      if (current) {
        set({ publishValidation: { ...current, can_publish: false } });
      }
    } finally {
      set({ reconcileLoading: false });
    }
  },

  publishModel: async (modelId) => {
    set({ validationLoading: true, error: null });
    try {
      const publishValidation = await publishOrgModel(modelId);
      set({ publishValidation });
    } catch (err) {
      // Step-up must bubble to `withStepUp` — do not treat it as a validation failure.
      const errStr =
        typeof err === "object" && err !== null
          ? JSON.stringify(err)
          : err instanceof Error
            ? err.message
            : String(err);
      if (errStr.includes("STEP_UP_REQUIRED") || errStr.includes("StepUpRequired")) {
        set({ validationLoading: false });
        throw err;
      }

      set({ error: formatOrgIpcError(err) });
      // Force publish off until a fresh validate succeeds.
      const current = get().publishValidation;
      if (current) {
        set({ publishValidation: { ...current, can_publish: false } });
      }
      try {
        const publishValidation = await validateOrgModelForPublish(modelId);
        set({ publishValidation });
      } catch {
        // Validation refresh failed — keep original error
      }
    } finally {
      set({ validationLoading: false });
    }
  },

  loadAuditEvents: async (limit, entityKind, entityId) => {
    set({ auditLoading: true, error: null });
    try {
      const auditEvents = await listOrgChangeEvents(limit, entityKind, entityId);
      set({ auditEvents });
    } catch (err) {
      set({ error: formatOrgIpcError(err) });
    } finally {
      set({ auditLoading: false });
    }
  },

  clearError: () => set({ error: null }),
}));

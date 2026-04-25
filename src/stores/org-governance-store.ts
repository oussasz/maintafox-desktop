/**
 * org-governance-store.ts
 *
 * UI state for publish-readiness validation and the org audit timeline (SP01-F04).
 */

import { create } from "zustand";

import {
  listOrgChangeEvents,
  publishOrgModel,
  validateOrgModelForPublish,
} from "@/services/org-governance-service";
import { toErrorMessage } from "@/utils/errors";
import type { OrgChangeEvent, OrgPublishValidationResult } from "@shared/ipc-types";

interface OrgGovernanceStoreState {
  publishValidation: OrgPublishValidationResult | null;
  validationLoading: boolean;
  auditEvents: OrgChangeEvent[];
  auditLoading: boolean;
  error: string | null;

  loadPublishValidation: (modelId: number) => Promise<void>;
  publishModel: (modelId: number) => Promise<void>;
  loadAuditEvents: (limit?: number, entityKind?: string, entityId?: number) => Promise<void>;
  clearError: () => void;
}

export const useOrgGovernanceStore = create<OrgGovernanceStoreState>()((set, _get) => ({
  publishValidation: null,
  validationLoading: false,
  auditEvents: [],
  auditLoading: false,
  error: null,

  loadPublishValidation: async (modelId) => {
    set({ validationLoading: true, error: null });
    try {
      const publishValidation = await validateOrgModelForPublish(modelId);
      set({ publishValidation });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ validationLoading: false });
    }
  },

  publishModel: async (modelId) => {
    set({ validationLoading: true, error: null });
    try {
      const publishValidation = await publishOrgModel(modelId);
      set({ publishValidation });
    } catch (err) {
      set({ error: toErrorMessage(err) });
      // Reload validation state so banner reflects current blockers
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
      set({ error: toErrorMessage(err) });
    } finally {
      set({ auditLoading: false });
    }
  },

  clearError: () => set({ error: null }),
}));

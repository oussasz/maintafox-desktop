/**
 * org-designer-store.ts
 *
 * Zustand store for the organization designer workspace.
 * Manages the designer snapshot, filters, selected-node context,
 * and impact-preview drawer state.
 */

import { create } from "zustand";

import { getOrgDesignerSnapshot, previewOrgChange } from "@/services/org-designer-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  OrgDesignerSnapshot,
  OrgImpactPreview,
  PreviewOrgChangePayload,
} from "@shared/ipc-types";

interface OrgDesignerStoreState {
  snapshot: OrgDesignerSnapshot | null;
  filterText: string;
  statusFilter: string | null;
  typeFilter: string | null;
  selectedNodeId: number | null;
  preview: OrgImpactPreview | null;
  previewOpen: boolean;
  loading: boolean;
  previewLoading: boolean;
  error: string | null;

  loadSnapshot: () => Promise<void>;
  setFilterText: (value: string) => void;
  setStatusFilter: (value: string | null) => void;
  setTypeFilter: (value: string | null) => void;
  setSelectedNodeId: (nodeId: number | null) => void;
  openPreview: (payload: PreviewOrgChangePayload) => Promise<void>;
  closePreview: () => void;
}

export const useOrgDesignerStore = create<OrgDesignerStoreState>()((set) => ({
  snapshot: null,
  filterText: "",
  statusFilter: null,
  typeFilter: null,
  selectedNodeId: null,
  preview: null,
  previewOpen: false,
  loading: false,
  previewLoading: false,
  error: null,

  loadSnapshot: async () => {
    set({ loading: true, error: null });
    try {
      const snapshot = await getOrgDesignerSnapshot();
      set({ snapshot });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  setFilterText: (value) => set({ filterText: value }),

  setStatusFilter: (value) => set({ statusFilter: value }),

  setTypeFilter: (value) => set({ typeFilter: value }),

  setSelectedNodeId: (nodeId) => set({ selectedNodeId: nodeId }),

  openPreview: async (payload) => {
    set({ previewLoading: true, previewOpen: true, preview: null });
    try {
      const preview = await previewOrgChange(payload);
      set({ preview });
    } catch (err) {
      set({
        error: toErrorMessage(err),
        previewOpen: false,
      });
    } finally {
      set({ previewLoading: false });
    }
  },

  closePreview: () => set({ preview: null, previewOpen: false }),
}));

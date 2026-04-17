/**
 * org-store.ts
 *
 * Zustand store for org configuration state.
 * Caches the active structure model and its node types for use throughout
 * the UI. Node-level state (the actual org tree) is in a separate store
 * added in SP01-F02.
 */

import { create } from "zustand";

import {
  getActiveOrgStructureModel,
  listOrgNodeTypes,
  listOrgRelationshipRules,
} from "@/services/org-service";
import type { OrgNodeType, OrgRelationshipRule, OrgStructureModel } from "@shared/ipc-types";

interface OrgConfigState {
  activeModel: OrgStructureModel | null;
  nodeTypes: OrgNodeType[];
  relationshipRules: OrgRelationshipRule[];
  loading: boolean;
  error: string | null;

  /** Load the active model and its node types + rules into the store. */
  loadActiveModelConfig: () => Promise<void>;
  /** Replace the active model after a publish operation. */
  setActiveModel: (model: OrgStructureModel) => void;
}

export const useOrgStore = create<OrgConfigState>()((set) => ({
  activeModel: null,
  nodeTypes: [],
  relationshipRules: [],
  loading: false,
  error: null,

  loadActiveModelConfig: async () => {
    set({ loading: true, error: null });
    try {
      const model = await getActiveOrgStructureModel();
      if (model) {
        const [types, rules] = await Promise.all([
          listOrgNodeTypes(model.id),
          listOrgRelationshipRules(model.id),
        ]);
        set({ activeModel: model, nodeTypes: types, relationshipRules: rules });
      } else {
        set({ activeModel: null, nodeTypes: [], relationshipRules: [] });
      }
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : String(err),
      });
    } finally {
      set({ loading: false });
    }
  },

  setActiveModel: (model) => set({ activeModel: model }),
}));

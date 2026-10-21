/**
 * org-node-store.ts
 *
 * Zustand store for the org tree, selected node context, responsibilities,
 * and entity bindings. Configuration-level state (structure model, node types,
 * relationship rules) remains in org-store.ts.
 */

import { create } from "zustand";

import {
  getOrgNode,
  listOrgEntityBindings,
  listOrgNodeResponsibilities,
  listOrgTree,
} from "@/services/org-node-service";
import { operationalNodeIdOf, useOrgDesignerStore } from "@/stores/org-designer-store";
import type {
  OrgEntityBinding,
  OrgNode,
  OrgNodeResponsibility,
  OrgTreeRow,
} from "@shared/ipc-types";

interface OrgNodeStoreState {
  treeRows: OrgTreeRow[];
  selectedNodeId: number | null;
  selectedNode: OrgNode | null;
  responsibilities: OrgNodeResponsibility[];
  bindings: OrgEntityBinding[];
  loading: boolean;
  saving: boolean;
  /** Last load failure (raw; render with ErrorState / describeAppError). */
  error: unknown;

  loadTree: () => Promise<void>;
  selectNode: (nodeId: number | null) => Promise<void>;
  refreshSelectedNodeContext: () => Promise<void>;
}

function currentDisplayModelId(): number | null {
  return useOrgDesignerStore.getState().snapshot?.display_model_id ?? null;
}

async function loadOperationalContext(
  nodeId: number,
): Promise<{ responsibilities: OrgNodeResponsibility[]; bindings: OrgEntityBinding[] }> {
  const opsId = operationalNodeIdOf(useOrgDesignerStore.getState().snapshot, nodeId);
  if (opsId == null) return { responsibilities: [], bindings: [] };
  const [responsibilities, bindings] = await Promise.all([
    listOrgNodeResponsibilities(opsId),
    listOrgEntityBindings(opsId),
  ]);
  return { responsibilities, bindings };
}

export const useOrgNodeStore = create<OrgNodeStoreState>()((set, get) => ({
  treeRows: [],
  selectedNodeId: null,
  selectedNode: null,
  responsibilities: [],
  bindings: [],
  loading: false,
  saving: false,
  error: null,

  loadTree: async () => {
    set({ loading: true, error: null });
    try {
      const treeRows = await listOrgTree();
      set({ treeRows });
    } catch (err) {
      set({ error: err });
    } finally {
      set({ loading: false });
    }
  },

  selectNode: async (nodeId) => {
    if (nodeId === null) {
      set({
        selectedNodeId: null,
        selectedNode: null,
        responsibilities: [],
        bindings: [],
        error: null,
      });
      return;
    }
    set({ loading: true, error: null, selectedNodeId: nodeId });
    try {
      const modelId = currentDisplayModelId();
      const [node, context] = await Promise.all([
        getOrgNode(nodeId, modelId),
        loadOperationalContext(nodeId),
      ]);
      set({ selectedNode: node, ...context });
    } catch (err) {
      set({ error: err });
    } finally {
      set({ loading: false });
    }
  },

  refreshSelectedNodeContext: async () => {
    const { selectedNodeId } = get();
    if (selectedNodeId === null) return;

    set({ loading: true, error: null });
    try {
      const modelId = currentDisplayModelId();
      const [treeRows, node, context] = await Promise.all([
        listOrgTree(),
        getOrgNode(selectedNodeId, modelId),
        loadOperationalContext(selectedNodeId),
      ]);
      set({ treeRows, selectedNode: node, ...context });
    } catch (err) {
      set({ error: err });
    } finally {
      set({ loading: false });
    }
  },
}));

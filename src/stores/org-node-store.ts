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
import { toErrorMessage } from "@/utils/errors";
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
  error: string | null;

  loadTree: () => Promise<void>;
  selectNode: (nodeId: number | null) => Promise<void>;
  refreshSelectedNodeContext: () => Promise<void>;
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
      set({ error: toErrorMessage(err) });
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
      const [node, responsibilities, bindings] = await Promise.all([
        getOrgNode(nodeId),
        listOrgNodeResponsibilities(nodeId),
        listOrgEntityBindings(nodeId),
      ]);
      set({ selectedNode: node, responsibilities, bindings });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  refreshSelectedNodeContext: async () => {
    const { selectedNodeId } = get();
    if (selectedNodeId === null) return;

    set({ loading: true, error: null });
    try {
      const [treeRows, node, responsibilities, bindings] = await Promise.all([
        listOrgTree(),
        getOrgNode(selectedNodeId),
        listOrgNodeResponsibilities(selectedNodeId),
        listOrgEntityBindings(selectedNodeId),
      ]);
      set({ treeRows, selectedNode: node, responsibilities, bindings });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },
}));

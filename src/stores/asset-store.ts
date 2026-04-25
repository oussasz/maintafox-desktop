/**
 * asset-store.ts
 *
 * Zustand store for the asset registry and hierarchy context.
 * Manages the asset list, selected asset detail, and hierarchy relations
 * for the active asset.
 */

import { create } from "zustand";

import {
  listAssets,
  getAssetById,
  createAsset,
  updateAssetIdentity,
  listAssetChildren,
  linkAssetHierarchy,
  unlinkAssetHierarchy,
  moveAssetOrgNode,
} from "@/services/asset-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  Asset,
  AssetHierarchyRow,
  CreateAssetPayload,
  UpdateAssetIdentityPayload,
  LinkAssetPayload,
} from "@shared/ipc-types";

interface AssetStoreState {
  list: Asset[];
  selectedAsset: Asset | null;
  hierarchy: AssetHierarchyRow[];
  loading: boolean;
  saving: boolean;
  error: string | null;

  // Form dialog state
  showCreateForm: boolean;
  showEditForm: boolean;
  editingAsset: Asset | null;
  parentPreFill: Asset | null;

  // Tree state
  treeMode: boolean;
  treeExpandedIds: Set<number>;
  treeSelectedId: number | null;
  treeChildren: Map<number, Asset[]>;
  treeRoots: Asset[];
  treeLoading: boolean;

  loadAssets: (
    statusFilter?: string | null,
    orgNodeFilter?: number | null,
    query?: string | null,
  ) => Promise<void>;
  selectAsset: (assetId: number | null) => Promise<void>;
  createAsset: (
    payload: CreateAssetPayload,
    opts?: { parentAssetId?: number | null },
  ) => Promise<Asset>;
  updateAsset: (
    assetId: number,
    payload: UpdateAssetIdentityPayload,
    expectedRowVersion: number,
  ) => Promise<Asset>;
  linkChild: (payload: LinkAssetPayload) => Promise<AssetHierarchyRow>;
  unlinkChild: (relationId: number, effectiveTo?: string | null) => Promise<AssetHierarchyRow>;
  moveAssetOrgNode: (
    assetId: number,
    newOrgNodeId: number,
    expectedRowVersion: number,
  ) => Promise<Asset>;

  // Form actions
  openCreateForm: (parent?: Asset | null) => void;
  closeCreateForm: () => void;
  openEditForm: (asset: Asset) => void;
  closeEditForm: () => void;

  // Tree actions
  setTreeMode: (enabled: boolean) => void;
  loadTreeRoots: () => Promise<void>;
  loadTreeChildren: (parentId: number) => Promise<void>;
  toggleTreeExpand: (nodeId: number) => void;
  selectTreeNode: (assetId: number | null) => void;
}

export const useAssetStore = create<AssetStoreState>()((set, get) => ({
  list: [],
  selectedAsset: null,
  hierarchy: [],
  loading: false,
  saving: false,
  error: null,

  // Form dialog state
  showCreateForm: false,
  showEditForm: false,
  editingAsset: null,
  parentPreFill: null,

  // Tree state
  treeMode: false,
  treeExpandedIds: new Set<number>(),
  treeSelectedId: null,
  treeChildren: new Map<number, Asset[]>(),
  treeRoots: [],
  treeLoading: false,

  loadAssets: async (statusFilter, orgNodeFilter, query) => {
    set({ loading: true, error: null });
    try {
      const list = await listAssets(statusFilter, orgNodeFilter, query);
      set({ list });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  selectAsset: async (assetId) => {
    if (assetId === null) {
      set({ selectedAsset: null, hierarchy: [], error: null });
      return;
    }
    set({ loading: true, error: null });
    try {
      const [asset, hierarchy] = await Promise.all([
        getAssetById(assetId),
        listAssetChildren(assetId),
      ]);
      set({ selectedAsset: asset, hierarchy });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  createAsset: async (payload, opts) => {
    set({ saving: true, error: null });
    try {
      const asset = await createAsset(payload);
      const parentId = opts?.parentAssetId;
      if (parentId != null && parentId > 0 && parentId !== asset.id) {
        await linkAssetHierarchy({
          parent_asset_id: parentId,
          child_asset_id: asset.id,
          relation_type: "PARENT_CHILD",
        });
      }
      // Reload list to include new asset
      const list = await listAssets();
      set({ list, selectedAsset: asset });
      return asset;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updateAsset: async (assetId, payload, expectedRowVersion) => {
    set({ saving: true, error: null });
    try {
      const asset = await updateAssetIdentity(assetId, payload, expectedRowVersion);
      const list = await listAssets();
      set({ list, selectedAsset: asset });
      return asset;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  linkChild: async (payload) => {
    set({ saving: true, error: null });
    try {
      const link = await linkAssetHierarchy(payload);
      // Reload hierarchy for the current selected asset
      const { selectedAsset } = get();
      if (selectedAsset) {
        const hierarchy = await listAssetChildren(selectedAsset.id);
        set({ hierarchy });
      }
      return link;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  unlinkChild: async (relationId, effectiveTo) => {
    set({ saving: true, error: null });
    try {
      const unlinked = await unlinkAssetHierarchy(relationId, effectiveTo);
      // Reload hierarchy for the current selected asset
      const { selectedAsset } = get();
      if (selectedAsset) {
        const hierarchy = await listAssetChildren(selectedAsset.id);
        set({ hierarchy });
      }
      return unlinked;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  moveAssetOrgNode: async (assetId, newOrgNodeId, expectedRowVersion) => {
    set({ saving: true, error: null });
    try {
      const asset = await moveAssetOrgNode(assetId, newOrgNodeId, expectedRowVersion);
      const list = await listAssets();
      set({ list, selectedAsset: asset });
      return asset;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  // ── Form actions ──────────────────────────────────────────────────────────

  openCreateForm: (parent) => {
    set({
      showCreateForm: true,
      showEditForm: false,
      editingAsset: null,
      parentPreFill: parent ?? null,
    });
  },

  closeCreateForm: () => {
    set({ showCreateForm: false, parentPreFill: null });
  },

  openEditForm: (asset) => {
    set({
      showEditForm: true,
      showCreateForm: false,
      editingAsset: asset,
    });
  },

  closeEditForm: () => {
    set({ showEditForm: false, editingAsset: null });
  },

  // ── Tree actions ──────────────────────────────────────────────────────────

  setTreeMode: (enabled) => {
    set({ treeMode: enabled });
    if (enabled && get().treeRoots.length === 0) {
      void get().loadTreeRoots();
    }
  },

  loadTreeRoots: async () => {
    set({ treeLoading: true, error: null });
    try {
      // Root assets have no parent — list all then filter client-side
      // (or use a null-parent filter if backend supports it)
      const all = await listAssets(null, null, null);
      // For now, use the full list as tree roots.
      // The tree navigator will lazy-load children via hierarchy.
      set({ treeRoots: all, treeLoading: false });
    } catch (err) {
      set({ error: toErrorMessage(err), treeLoading: false });
    }
  },

  loadTreeChildren: async (parentId) => {
    try {
      const rows = await listAssetChildren(parentId);
      // Resolve each child to a full Asset object
      const children = await Promise.all(rows.map((r) => getAssetById(r.child_asset_id)));
      const map = new Map(get().treeChildren);
      map.set(parentId, children);
      set({ treeChildren: map });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  toggleTreeExpand: (nodeId) => {
    const expanded = new Set(get().treeExpandedIds);
    if (expanded.has(nodeId)) {
      expanded.delete(nodeId);
    } else {
      expanded.add(nodeId);
      // Trigger child load if not yet loaded
      if (!get().treeChildren.has(nodeId)) {
        void get().loadTreeChildren(nodeId);
      }
    }
    set({ treeExpandedIds: expanded });
  },

  selectTreeNode: (assetId) => {
    set({ treeSelectedId: assetId });
  },
}));

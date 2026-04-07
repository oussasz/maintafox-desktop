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

  loadAssets: (
    statusFilter?: string | null,
    orgNodeFilter?: number | null,
    query?: string | null,
  ) => Promise<void>;
  selectAsset: (assetId: number | null) => Promise<void>;
  createAsset: (payload: CreateAssetPayload) => Promise<Asset>;
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
}

export const useAssetStore = create<AssetStoreState>()((set, get) => ({
  list: [],
  selectedAsset: null,
  hierarchy: [],
  loading: false,
  saving: false,
  error: null,

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

  createAsset: async (payload) => {
    set({ saving: true, error: null });
    try {
      const asset = await createAsset(payload);
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
}));

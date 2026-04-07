/**
 * asset-service.ts
 *
 * IPC wrappers for asset identity, classification, and hierarchy commands.
 * All invoke() calls for SP02-F01 asset operations are isolated here.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  Asset,
  AssetHierarchyRow,
  CreateAssetPayload,
  UpdateAssetIdentityPayload,
  LinkAssetPayload,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const AssetSchema = z.object({
  id: z.number(),
  sync_id: z.string(),
  asset_code: z.string(),
  asset_name: z.string(),
  class_id: z.number().nullable(),
  class_code: z.string().nullable(),
  class_name: z.string().nullable(),
  family_code: z.string().nullable(),
  family_name: z.string().nullable(),
  criticality_value_id: z.number().nullable(),
  criticality_code: z.string().nullable(),
  status_code: z.string(),
  manufacturer: z.string().nullable(),
  model: z.string().nullable(),
  serial_number: z.string().nullable(),
  maintainable_boundary: z.boolean(),
  org_node_id: z.number().nullable(),
  org_node_name: z.string().nullable(),
  commissioned_at: z.string().nullable(),
  decommissioned_at: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  deleted_at: z.string().nullable(),
  row_version: z.number(),
});

export const AssetHierarchyRowSchema = z.object({
  relation_id: z.number(),
  parent_asset_id: z.number(),
  child_asset_id: z.number(),
  relation_type: z.string(),
  effective_from: z.string().nullable(),
  effective_to: z.string().nullable(),
});

// ── Error helpers ─────────────────────────────────────────────────────────────

interface IpcError {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcError {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class VersionConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "VersionConflictError";
  }
}

function rethrowIfVersionConflict(err: unknown): never {
  if (isIpcError(err) && err.code === "VALIDATION_FAILED" && err.message.includes("version")) {
    throw new VersionConflictError(err.message);
  }
  throw err;
}

// ── Identity commands ─────────────────────────────────────────────────────────

export async function listAssets(
  statusFilter?: string | null,
  orgNodeFilter?: number | null,
  query?: string | null,
  limit?: number | null,
): Promise<Asset[]> {
  const raw = await invoke<unknown>("list_assets", {
    statusFilter: statusFilter ?? null,
    orgNodeFilter: orgNodeFilter ?? null,
    query: query ?? null,
    limit: limit ?? null,
  });
  return z.array(AssetSchema).parse(raw) as Asset[];
}

export async function getAssetById(assetId: number): Promise<Asset> {
  const raw = await invoke<unknown>("get_asset_by_id", { assetId });
  return AssetSchema.parse(raw) as Asset;
}

export async function createAsset(payload: CreateAssetPayload): Promise<Asset> {
  const raw = await invoke<unknown>("create_asset", { payload });
  return AssetSchema.parse(raw) as Asset;
}

export async function updateAssetIdentity(
  assetId: number,
  payload: UpdateAssetIdentityPayload,
  expectedRowVersion: number,
): Promise<Asset> {
  try {
    const raw = await invoke<unknown>("update_asset_identity", {
      assetId,
      payload,
      expectedRowVersion,
    });
    return AssetSchema.parse(raw) as Asset;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// ── Hierarchy commands ────────────────────────────────────────────────────────

export async function listAssetChildren(parentAssetId: number): Promise<AssetHierarchyRow[]> {
  const raw = await invoke<unknown>("list_asset_children", { parentAssetId });
  return z.array(AssetHierarchyRowSchema).parse(raw) as AssetHierarchyRow[];
}

export async function listAssetParents(childAssetId: number): Promise<AssetHierarchyRow[]> {
  const raw = await invoke<unknown>("list_asset_parents", { childAssetId });
  return z.array(AssetHierarchyRowSchema).parse(raw) as AssetHierarchyRow[];
}

export async function linkAssetHierarchy(payload: LinkAssetPayload): Promise<AssetHierarchyRow> {
  const raw = await invoke<unknown>("link_asset_hierarchy", { payload });
  return AssetHierarchyRowSchema.parse(raw) as AssetHierarchyRow;
}

export async function unlinkAssetHierarchy(
  relationId: number,
  effectiveTo?: string | null,
): Promise<AssetHierarchyRow> {
  const raw = await invoke<unknown>("unlink_asset_hierarchy", {
    relationId,
    effectiveTo: effectiveTo ?? null,
  });
  return AssetHierarchyRowSchema.parse(raw) as AssetHierarchyRow;
}

export async function moveAssetOrgNode(
  assetId: number,
  newOrgNodeId: number,
  expectedRowVersion: number,
): Promise<Asset> {
  try {
    const raw = await invoke<unknown>("move_asset_org_node", {
      assetId,
      newOrgNodeId,
      expectedRowVersion,
    });
    return AssetSchema.parse(raw) as Asset;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

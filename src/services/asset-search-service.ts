/**
 * asset-search-service.ts
 *
 * IPC wrappers for the asset search and suggestion commands.
 * All invoke() calls for SP02-F03 search operations are isolated here.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { AssetSearchFilters, AssetSearchResult, AssetSuggestion } from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const AssetSearchResultSchema = z.object({
  id: z.number(),
  sync_id: z.string(),
  asset_code: z.string(),
  asset_name: z.string(),
  class_code: z.string().nullable(),
  class_name: z.string().nullable(),
  family_code: z.string().nullable(),
  family_name: z.string().nullable(),
  criticality_code: z.string().nullable(),
  status_code: z.string(),
  org_node_id: z.number().nullable(),
  org_node_name: z.string().nullable(),
  parent_asset_id: z.number().nullable(),
  parent_asset_code: z.string().nullable(),
  parent_asset_name: z.string().nullable(),
  primary_meter_name: z.string().nullable(),
  primary_meter_reading: z.number().nullable(),
  primary_meter_unit: z.string().nullable(),
  primary_meter_last_read_at: z.string().nullable(),
  external_id_count: z.number(),
  row_version: z.number(),
});

export const AssetSuggestionSchema = z.object({
  id: z.number(),
  asset_code: z.string(),
  asset_name: z.string(),
  status_code: z.string(),
});

// ── Search command ────────────────────────────────────────────────────────────

export async function searchAssets(filters: AssetSearchFilters): Promise<AssetSearchResult[]> {
  const raw = await invoke<unknown>("search_assets", {
    filters: {
      query: filters.query ?? null,
      classCodes: filters.classCodes ?? null,
      familyCodes: filters.familyCodes ?? null,
      statusCodes: filters.statusCodes ?? null,
      orgNodeIds: filters.orgNodeIds ?? null,
      includeDecommissioned: filters.includeDecommissioned ?? null,
      limit: filters.limit ?? null,
    },
  });
  return z.array(AssetSearchResultSchema).parse(raw) as AssetSearchResult[];
}

// ── Suggestion commands ───────────────────────────────────────────────────────

export async function suggestAssetCodes(
  prefix: string,
  limit?: number | null,
): Promise<AssetSuggestion[]> {
  const raw = await invoke<unknown>("suggest_asset_codes", {
    prefix,
    limit: limit ?? null,
  });
  return z.array(AssetSuggestionSchema).parse(raw) as AssetSuggestion[];
}

export async function suggestAssetNames(
  partial: string,
  limit?: number | null,
): Promise<AssetSuggestion[]> {
  const raw = await invoke<unknown>("suggest_asset_names", {
    partial,
    limit: limit ?? null,
  });
  return z.array(AssetSuggestionSchema).parse(raw) as AssetSuggestion[];
}

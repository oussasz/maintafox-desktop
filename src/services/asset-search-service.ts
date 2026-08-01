/**
 * asset-search-service.ts
 *
 * IPC wrappers for the asset search and suggestion commands.
 * All invoke() calls for SP02-F03 search operations are isolated here.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AssetPickerSuggestFilters,
  AssetPickerSuggestions,
  AssetSearchFilters,
  AssetSearchResult,
  AssetSuggestion,
} from "@shared/ipc-types";

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
  subfamily_code: z.string().nullable(),
  subfamily_name: z.string().nullable(),
  criticality_code: z.string().nullable(),
  status_code: z.string(),
  org_node_id: z.number().nullable(),
  org_node_name: z.string().nullable(),
  org_path: z.string().nullish(),
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

export const AssetPickerSuggestionsSchema = z.object({
  mode: z.string(),
  items: z.array(AssetSearchResultSchema),
});

// ── Search command ────────────────────────────────────────────────────────────

export async function searchAssets(filters: AssetSearchFilters): Promise<AssetSearchResult[]> {
  const raw = await invoke<unknown>("search_assets", { filters });
  return z.array(AssetSearchResultSchema).parse(raw) as AssetSearchResult[];
}

/** Empty-query recent/frequent (or typed search) for the shared AssetPicker. */
export async function suggestPickerAssets(
  filters: AssetPickerSuggestFilters = {},
): Promise<AssetPickerSuggestions> {
  const raw = await invoke<unknown>("suggest_picker_assets", { filters });
  return AssetPickerSuggestionsSchema.parse(raw) as AssetPickerSuggestions;
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

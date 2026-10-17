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
  AssetSearchPage,
  AssetSearchResult,
  AssetSuggestion,
  EquipmentRegistryMetricsPayload,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const AssetSearchResultSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
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

const AssetTreeLevelSchema = z.object({
  parent_id: z.number().nullable(),
  items: z.array(
    AssetSearchResultSchema.extend({ child_count: z.number(), is_match: z.boolean() }),
  ),
  total: z.number(),
});

/** One asset of an equipment tree level: the asset, its visible children, whether it matches. */
export type AssetTreeRow = AssetSearchResult & { child_count: number; is_match: boolean };
export interface AssetTreeLevel {
  parent_id: number | null;
  items: AssetTreeRow[];
  total: number;
}

/**
 * One level of the equipment tree read from the hierarchy (roots when `parentId` is null),
 * within the registry filters: matching assets and their ancestors (`assets::tree`).
 */
export async function listAssetTreeLevel(
  filters: AssetSearchFilters,
  parentId: number | null,
): Promise<AssetTreeLevel> {
  const raw = await invoke<unknown>("list_asset_tree_level", { filters, parentId });
  return AssetTreeLevelSchema.parse(raw) as AssetTreeLevel;
}

/** One page of the registry list plus the number of assets matching the same filters. */
export async function searchAssetsPage(filters: AssetSearchFilters): Promise<AssetSearchPage> {
  const raw = await invoke<unknown>("search_assets_page", { filters });
  return z
    .object({ items: z.array(AssetSearchResultSchema), total: z.number() })
    .parse(raw) as AssetSearchPage;
}

const EquipmentRegistryMetricCellSchema = z.object({
  semantic_key: z.string(),
  catalog_id: z.string(),
  value: z.number(),
  available: z.boolean(),
  value_kind: z.enum(["count", "percent"]),
  trend_sense: z.enum(["up-good", "up-bad", "neutral"]),
  active_count: z.number().nullable().optional(),
  down_count: z.number().nullable().optional(),
  series: z.array(z.number()).nullable().optional(),
});

const EquipmentRegistryMetricsPayloadSchema = z.object({
  scoped_total: z.number(),
  cells: z.array(EquipmentRegistryMetricCellSchema),
});

/** Filter-scoped Equipment registry KPI strip metrics (same filters as search_assets). */
export async function getEquipmentRegistryMetrics(
  filters: AssetSearchFilters,
): Promise<EquipmentRegistryMetricsPayload> {
  const raw = await invoke<unknown>("get_equipment_registry_metrics", { filters });
  return EquipmentRegistryMetricsPayloadSchema.parse(raw) as EquipmentRegistryMetricsPayload;
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

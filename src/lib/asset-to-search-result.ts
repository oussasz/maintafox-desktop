import type { Asset, AssetSearchResult } from "@shared/ipc-types";

/** Map a full `Asset` row into the combobox/search shape used by DI/WO create forms. */
export function assetToSearchResult(
  asset: Asset,
  extras?: Partial<
    Pick<AssetSearchResult, "parent_asset_id" | "parent_asset_code" | "parent_asset_name">
  >,
): AssetSearchResult {
  return {
    id: asset.id,
    sync_id: asset.sync_id,
    asset_code: asset.asset_code,
    asset_name: asset.asset_name,
    class_code: asset.class_code,
    class_name: asset.class_name,
    family_code: asset.family_code,
    family_name: asset.family_name,
    subfamily_code: asset.subfamily_code,
    subfamily_name: asset.subfamily_name,
    criticality_code: asset.criticality_code,
    status_code: asset.status_code,
    org_node_id: asset.org_node_id,
    org_node_name: asset.org_node_name,
    parent_asset_id: extras?.parent_asset_id ?? null,
    parent_asset_code: extras?.parent_asset_code ?? null,
    parent_asset_name: extras?.parent_asset_name ?? null,
    primary_meter_name: null,
    primary_meter_reading: null,
    primary_meter_unit: null,
    primary_meter_last_read_at: null,
    external_id_count: 0,
    row_version: asset.row_version,
  };
}

/**
 * "Transférer" (L2-04): an asset moves to another org node with a reason and an effective date,
 * recorded as its MOVED lifecycle event (who, when, from, to) in one server transaction. The
 * preview gives what stays attached to the asset and the nodes the person may transfer it to.
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { AssetLifecycleEventSchema } from "@/services/asset-lifecycle-service";
import { AssetSchema } from "@/services/asset-service";
import type { Asset, AssetLifecycleEvent } from "@shared/ipc-types";

const AssetTransferImpactSchema = z.object({
  active_pm_plan_count: z.number(),
  spare_part_line_count: z.number(),
  critical_spare_part_count: z.number(),
  open_wo_count: z.number(),
});
export type AssetTransferImpact = z.infer<typeof AssetTransferImpactSchema>;

const TransferDestinationSchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  node_type_label: z.string(),
});
export type TransferDestination = z.infer<typeof TransferDestinationSchema>;

const AssetTransferPreviewSchema = z.object({
  asset_id: z.number(),
  from_org_node_id: z.number().nullable(),
  impact: AssetTransferImpactSchema,
  destinations: z.array(TransferDestinationSchema),
});
export type AssetTransferPreview = z.infer<typeof AssetTransferPreviewSchema>;

export interface TransferAssetPayload {
  asset_id: number;
  to_org_node_id: number;
  /** `YYYY-MM-DD`, never in the future. */
  effective_date: string;
  reason: string;
  /** The asset's row version the person saw. */
  expected_row_version: number;
}

export interface TransferAssetResult {
  asset: Asset;
  event: AssetLifecycleEvent;
  impact: AssetTransferImpact;
}

const TransferAssetResultSchema = z.object({
  asset: AssetSchema,
  event: AssetLifecycleEventSchema,
  impact: AssetTransferImpactSchema,
});

export async function previewAssetTransfer(assetId: number): Promise<AssetTransferPreview> {
  const raw = await invoke<unknown>("preview_asset_transfer", { assetId });
  return AssetTransferPreviewSchema.parse(raw);
}

export async function transferAsset(payload: TransferAssetPayload): Promise<TransferAssetResult> {
  const raw = await invoke<unknown>("transfer_asset", { payload });
  return TransferAssetResultSchema.parse(raw) as TransferAssetResult;
}

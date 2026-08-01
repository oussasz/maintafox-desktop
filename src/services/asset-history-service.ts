import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { AssetHistoryEvent, AssetHistoryQuery, AssetHistorySummary } from "@shared/ipc-types";

const AssetHistoryEventRefSchema = z.object({
  entity_type: z.enum(["asset", "wo", "di", "pm", "inspection", "document", "photo"]),
  entity_id: z.number().nullable(),
  entity_code: z.string().nullable(),
  route_hint: z.string().nullable(),
});

export const AssetHistoryEventSchema = z.object({
  id: z.string(),
  asset_id: z.number(),
  event_type: z.string(),
  occurred_at: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  actor_label: z.string().nullable(),
  duration_minutes: z.number().nullable(),
  status_label: z.string().nullable(),
  metadata: z.record(z.union([z.string(), z.number(), z.boolean(), z.null()])).default({}),
  ref: AssetHistoryEventRefSchema.nullable(),
});

export const AssetHistorySummarySchema = z.object({
  asset_id: z.number(),
  created_at: z.string().nullable(),
  age_days: z.number().nullable(),
  wo_count: z.number(),
  pm_count: z.number(),
  failure_count: z.number(),
  availability_percent: z.number().nullable(),
});

export async function listAssetHistoryEvents(
  query: AssetHistoryQuery,
): Promise<AssetHistoryEvent[]> {
  const raw = await invoke<unknown>("list_asset_history_events", { query });
  return z.array(AssetHistoryEventSchema).parse(raw) as AssetHistoryEvent[];
}

export async function getAssetHistorySummary(assetId: number): Promise<AssetHistorySummary> {
  const raw = await invoke<unknown>("get_asset_history_summary", { assetId });
  return AssetHistorySummarySchema.parse(raw) as AssetHistorySummary;
}

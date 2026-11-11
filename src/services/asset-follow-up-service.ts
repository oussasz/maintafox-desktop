/**
 * asset-follow-up-service.ts
 *
 * IPC wrappers for the technical characteristics of an equipment (defined per family in Données
 * de référence, L2-07) and its repeat-failure follow-up: flag, "5 pourquoi" records and tracked
 * corrective actions (L2-09).
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AssetCharacteristicSheet,
  AssetRepeatFailureStatus,
  CorrectiveAction,
  FiveWhyAnalysis,
  RepeatFailureOverview,
  SaveAssetCharacteristicsPayload,
  SaveCorrectiveActionPayload,
  SaveFiveWhyPayload,
} from "@shared/ipc-types";

const AssetCharacteristicSchema = z.object({
  code: z.string(),
  label: z.string(),
  data_type: z.enum(["number", "text", "boolean"]).nullable(),
  unit_code: z.string().nullable(),
  unit_symbol: z.string().nullable(),
  required: z.boolean(),
  definition_problem: z.string().nullable(),
  value: z.string().nullable(),
});

export const AssetCharacteristicSheetSchema = z.object({
  asset_id: z.number(),
  family_code: z.string().nullable(),
  family_label: z.string().nullable(),
  items: z.array(AssetCharacteristicSchema),
  unlisted: z.array(z.object({ code: z.string(), value: z.string() })),
});

const RepeatFailureRuleSchema = z.object({
  min_failures: z.number(),
  window_days: z.number(),
  since: z.string(),
});

export const RepeatFailureOverviewSchema = z.object({
  rule: RepeatFailureRuleSchema,
  assets: z.array(
    z.object({
      asset_id: z.number(),
      asset_code: z.string(),
      asset_name: z.string(),
      failure_count: z.number(),
      last_failure_at: z.string().nullable(),
    }),
  ),
});

const AssetRepeatFailureStatusSchema = z.object({
  asset_id: z.number(),
  rule: RepeatFailureRuleSchema,
  failure_count: z.number(),
  last_failure_at: z.string().nullable(),
  is_repeat_failure: z.boolean(),
  open_action_count: z.number(),
  overdue_action_count: z.number(),
});

const FiveWhySchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  problem_statement: z.string(),
  why_1: z.string().nullable(),
  why_2: z.string().nullable(),
  why_3: z.string().nullable(),
  why_4: z.string().nullable(),
  why_5: z.string().nullable(),
  root_cause: z.string().nullable(),
  created_by_name: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const CorrectiveActionSchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  action_text: z.string(),
  owner_personnel_id: z.number().nullable(),
  owner_name: z.string().nullable(),
  owner_employee_code: z.string().nullable(),
  due_date: z.string().nullable(),
  status: z.enum(["open", "done", "cancelled"]),
  completed_at: z.string().nullable(),
  is_overdue: z.boolean(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

// ── Technical characteristics ─────────────────────────────────────────────

export async function getAssetCharacteristics(assetId: number): Promise<AssetCharacteristicSheet> {
  const raw = await invoke<unknown>("get_asset_characteristics", { assetId });
  return AssetCharacteristicSheetSchema.parse(raw) as AssetCharacteristicSheet;
}

export async function saveAssetCharacteristics(
  payload: SaveAssetCharacteristicsPayload,
): Promise<AssetCharacteristicSheet> {
  const raw = await invoke<unknown>("save_asset_characteristics", { payload });
  return AssetCharacteristicSheetSchema.parse(raw) as AssetCharacteristicSheet;
}

// ── Repeat failures ───────────────────────────────────────────────────────

/** Every equipment flagged "pannes répétitives" now, with the rule from the settings. */
export async function listRepeatFailureAssets(): Promise<RepeatFailureOverview> {
  const raw = await invoke<unknown>("list_repeat_failure_assets");
  return RepeatFailureOverviewSchema.parse(raw) as RepeatFailureOverview;
}

export async function getAssetRepeatFailureStatus(
  assetId: number,
): Promise<AssetRepeatFailureStatus> {
  const raw = await invoke<unknown>("get_asset_repeat_failure_status", { assetId });
  return AssetRepeatFailureStatusSchema.parse(raw) as AssetRepeatFailureStatus;
}

export async function listAssetFiveWhys(assetId: number): Promise<FiveWhyAnalysis[]> {
  const raw = await invoke<unknown>("list_asset_five_whys", { assetId });
  return z.array(FiveWhySchema).parse(raw) as FiveWhyAnalysis[];
}

export async function saveAssetFiveWhy(payload: SaveFiveWhyPayload): Promise<FiveWhyAnalysis> {
  const raw = await invoke<unknown>("save_asset_five_why", { payload });
  return FiveWhySchema.parse(raw) as FiveWhyAnalysis;
}

export async function listAssetCorrectiveActions(assetId: number): Promise<CorrectiveAction[]> {
  const raw = await invoke<unknown>("list_asset_corrective_actions", { assetId });
  return z.array(CorrectiveActionSchema).parse(raw) as CorrectiveAction[];
}

export async function saveAssetCorrectiveAction(
  payload: SaveCorrectiveActionPayload,
): Promise<CorrectiveAction> {
  const raw = await invoke<unknown>("save_asset_corrective_action", { payload });
  return CorrectiveActionSchema.parse(raw) as CorrectiveAction;
}

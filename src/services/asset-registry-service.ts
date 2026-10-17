/**
 * asset-registry-service.ts
 *
 * IPC wrappers for the asset record beyond identity: commercial / warranty data, the maintenance
 * profile (costs and failures), the spare-parts list and criticality assessments.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { AssetSchema } from "@/services/asset-service";
import type {
  ArticleAssetUsage,
  AssessCriticalityResult,
  Asset,
  AssetCommercialInput,
  AssetMaintenanceProfile,
  AssetSparePart,
  CriticalityAssessment,
  CriticalityMethod,
  CriticalityResult,
  CriticalityScores,
  SparePartInput,
} from "@shared/ipc-types";

const CostBreakdownSchema = z.object({
  labor: z.number(),
  parts: z.number(),
  services: z.number(),
  total: z.number(),
});

const MaintenanceProfileSchema = z.object({
  asset_id: z.number(),
  include_descendants: z.boolean(),
  asset_count: z.number(),
  work_order_count: z.number(),
  open_work_order_count: z.number(),
  failure_work_order_count: z.number(),
  planned_work_order_count: z.number(),
  downtime_hours: z.number(),
  last_failure_at: z.string().nullable(),
  cost_lifetime: CostBreakdownSchema,
  cost_recent: CostBreakdownSchema,
  recent_window_months: z.number(),
  cost_by_year: z.array(
    z.object({ year: z.number(), total: z.number(), work_order_count: z.number() }),
  ),
  replacement_value: z.number().nullable(),
  cost_to_replacement_ratio: z.number().nullable(),
  pm_plans: z.array(
    z.object({
      id: z.number(),
      code: z.string(),
      title: z.string(),
      lifecycle_status: z.string(),
      next_due_at: z.string().nullable(),
    }),
  ),
});

const SparePartSchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  unit_label: z.string().nullable(),
  article_is_active: z.boolean(),
  quantity: z.number(),
  position_label: z.string().nullable(),
  is_critical_spare: z.boolean(),
  notes: z.string().nullable(),
  on_hand_qty: z.number(),
  available_qty: z.number(),
  min_stock: z.number(),
  row_version: z.number(),
});

const CriticalityMethodSchema = z.object({
  code: z.string(),
  score_min: z.number(),
  score_max: z.number(),
  class_a_min_risk: z.number(),
  class_b_min_risk: z.number(),
  class_c_min_risk: z.number(),
  hse_override_score: z.number(),
});

const CriticalityResultSchema = z.object({
  method_code: z.string(),
  consequence: z.number(),
  risk_score: z.number(),
  resulting_class: z.enum(["A", "B", "C", "D"]),
  hse_override: z.boolean(),
});

const CriticalityAssessmentSchema = z.object({
  id: z.number(),
  asset_id: z.number(),
  method_code: z.string(),
  safety_score: z.number(),
  environment_score: z.number(),
  production_score: z.number(),
  quality_score: z.number(),
  cost_score: z.number(),
  likelihood_score: z.number(),
  risk_score: z.number(),
  resulting_class: z.string(),
  previous_class: z.string().nullable(),
  rationale: z.string().nullable(),
  assessed_by_name: z.string().nullable(),
  assessed_at: z.string(),
});

// ── Commercial / warranty ─────────────────────────────────────────────────────

export async function updateAssetCommercial(
  assetId: number,
  input: AssetCommercialInput,
  expectedRowVersion: number,
): Promise<Asset> {
  const raw = await invoke<unknown>("update_asset_commercial", {
    assetId,
    input,
    expectedRowVersion,
  });
  return AssetSchema.parse(raw) as Asset;
}

// ── Maintenance profile ───────────────────────────────────────────────────────

export async function getAssetMaintenanceProfile(
  assetId: number,
  includeDescendants: boolean,
): Promise<AssetMaintenanceProfile> {
  const raw = await invoke<unknown>("get_asset_maintenance_profile", {
    assetId,
    includeDescendants,
  });
  return MaintenanceProfileSchema.parse(raw) as AssetMaintenanceProfile;
}

// ── Spare parts ───────────────────────────────────────────────────────────────

export async function listAssetSpareParts(assetId: number): Promise<AssetSparePart[]> {
  const raw = await invoke<unknown>("list_asset_spare_parts", { assetId });
  return z.array(SparePartSchema).parse(raw) as AssetSparePart[];
}

export async function listArticleAssetUsage(articleId: number): Promise<ArticleAssetUsage[]> {
  const raw = await invoke<unknown>("list_article_asset_usage", { articleId });
  return z
    .array(
      z.object({
        asset_id: z.number(),
        asset_code: z.string(),
        asset_name: z.string(),
        quantity: z.number(),
        is_critical_spare: z.boolean(),
      }),
    )
    .parse(raw) as ArticleAssetUsage[];
}

export async function addAssetSparePart(
  assetId: number,
  articleId: number,
  line: SparePartInput,
): Promise<AssetSparePart> {
  const raw = await invoke<unknown>("add_asset_spare_part", {
    payload: { asset_id: assetId, article_id: articleId, line },
  });
  return SparePartSchema.parse(raw) as AssetSparePart;
}

export async function updateAssetSparePart(
  id: number,
  line: SparePartInput,
  expectedRowVersion: number,
): Promise<AssetSparePart> {
  const raw = await invoke<unknown>("update_asset_spare_part", { id, line, expectedRowVersion });
  return SparePartSchema.parse(raw) as AssetSparePart;
}

export async function removeAssetSparePart(id: number, expectedRowVersion: number): Promise<void> {
  await invoke<void>("remove_asset_spare_part", { id, expectedRowVersion });
}

// ── Criticality ───────────────────────────────────────────────────────────────

export async function getAssetCriticalityMethod(): Promise<CriticalityMethod> {
  const raw = await invoke<unknown>("get_asset_criticality_method", {});
  return CriticalityMethodSchema.parse(raw) as CriticalityMethod;
}

export async function previewAssetCriticality(
  scores: CriticalityScores,
): Promise<CriticalityResult> {
  const raw = await invoke<unknown>("preview_asset_criticality", { scores });
  return CriticalityResultSchema.parse(raw) as CriticalityResult;
}

export async function listAssetCriticalityAssessments(
  assetId: number,
): Promise<CriticalityAssessment[]> {
  const raw = await invoke<unknown>("list_asset_criticality_assessments", { assetId });
  return z.array(CriticalityAssessmentSchema).parse(raw) as CriticalityAssessment[];
}

export async function assessAssetCriticality(
  assetId: number,
  scores: CriticalityScores,
  rationale: string | null,
  expectedRowVersion: number,
): Promise<AssessCriticalityResult> {
  const raw = await invoke<unknown>("assess_asset_criticality", {
    payload: {
      asset_id: assetId,
      scores,
      rationale,
      expected_row_version: expectedRowVersion,
    },
  });
  return z
    .object({ assessment: CriticalityAssessmentSchema, asset: AssetSchema })
    .parse(raw) as AssessCriticalityResult;
}

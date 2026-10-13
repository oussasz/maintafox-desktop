import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AnalyticsDashboardLayoutPayload,
  AnalyticsExecutiveDashboard,
  AnalyticsExecutiveFilter,
  SaveAnalyticsDashboardLayoutInput,
} from "@shared/ipc-types";

const AnalyticsKpiValueSchema = z.object({
  key: z.string(),
  value: z.number(),
  previous_value: z.number(),
  available: z.boolean(),
  quality_badge: z.string().nullable(),
  catalog_id: z.string().nullable(),
});

const AnalyticsExecutiveDashboardSchema = z.object({
  horizon_days: z.number(),
  org_node_id: z.number().nullable(),
  open_dis: AnalyticsKpiValueSchema,
  open_wos: AnalyticsKpiValueSchema,
  total_assets: AnalyticsKpiValueSchema,
  overdue_items: AnalyticsKpiValueSchema,
  pm_overdue: AnalyticsKpiValueSchema,
  pm_due_today: AnalyticsKpiValueSchema,
  pm_due_next_7d: AnalyticsKpiValueSchema,
  avg_mtbf_hours: AnalyticsKpiValueSchema,
  avg_mttr_hours: AnalyticsKpiValueSchema,
  avg_availability: AnalyticsKpiValueSchema,
  avg_data_quality: AnalyticsKpiValueSchema,
  maintenance_cost: AnalyticsKpiValueSchema,
  reliability_snapshot_count: z.number(),
  reliability_total_events: z.number(),
  workload_days: z.array(
    z.object({
      date: z.string(),
      di_created: z.number(),
      wo_completed: z.number(),
      pm_due: z.number(),
    }),
  ),
  workload_quality_badge: z.string().nullable(),
  di_status_segments: z.array(
    z.object({
      status: z.string(),
      count: z.number(),
    }),
  ),
  reliability_series: z.array(
    z.object({
      date: z.string(),
      avg_mtbf: z.number().nullable(),
      avg_mttr: z.number().nullable(),
      avg_availability: z.number().nullable(),
      avg_data_quality: z.number().nullable(),
      maintenance_cost: z.number().nullable(),
    }),
  ),
  bad_actor_assets: z.array(
    z.object({
      equipment_id: z.number(),
      asset_code: z.string(),
      asset_label: z.string(),
      event_count: z.number(),
      mtbf_hours: z.number().nullable(),
      availability: z.number().nullable(),
      maintenance_cost: z.number(),
      cost_available: z.boolean(),
    }),
  ),
});

export async function getAnalyticsExecutiveDashboard(
  filter: AnalyticsExecutiveFilter,
): Promise<AnalyticsExecutiveDashboard> {
  const raw = await invoke<unknown>("get_analytics_executive_dashboard", { filter });
  return AnalyticsExecutiveDashboardSchema.parse(raw);
}

export async function getAnalyticsDashboardLayout(): Promise<AnalyticsDashboardLayoutPayload> {
  const raw = await invoke<unknown>("get_analytics_dashboard_layout");
  return z.object({ layout_json: z.string() }).parse(raw);
}

export async function saveAnalyticsDashboardLayout(
  input: SaveAnalyticsDashboardLayoutInput,
): Promise<void> {
  await invoke("save_analytics_dashboard_layout", { input });
}

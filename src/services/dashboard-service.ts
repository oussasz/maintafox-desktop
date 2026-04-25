// ADR-003: all invoke() calls live exclusively in src/services/.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  DashboardDiStatusChart,
  DashboardKpiValidation,
  DashboardKpis,
  DashboardLayoutPayload,
  DashboardReliabilitySnapshotSummary,
  DashboardWorkloadChart,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const KpiValueSchema = z.object({
  key: z.string(),
  value: z.number(),
  previous_value: z.number(),
  available: z.boolean(),
  quality_badge: z.string().nullable().optional(),
});

const DashboardKpisSchema = z.object({
  open_dis: KpiValueSchema,
  open_wos: KpiValueSchema,
  total_assets: KpiValueSchema,
  overdue_items: KpiValueSchema,
});

const WorkloadDaySchema = z.object({
  date: z.string(),
  di_created: z.number(),
  wo_completed: z.number(),
  pm_due: z.number(),
});

const DashboardWorkloadChartSchema = z.object({
  days: z.array(WorkloadDaySchema),
  period_days: z.number(),
  quality_badge: z.string().nullable().optional(),
});

const DashboardLayoutPayloadSchema = z.object({
  layout_json: z.string(),
});

const DashboardDiStatusChartSchema = z.object({
  segments: z.array(
    z.object({
      status: z.string(),
      count: z.number(),
    }),
  ),
  available: z.boolean(),
});

const DashboardReliabilitySnapshotSummarySchema = z.object({
  available: z.boolean(),
  snapshot_count: z.number(),
  avg_data_quality_score: z.number().nullable().optional(),
  avg_mtbf_hours: z.number().nullable().optional(),
  total_failure_events: z.number(),
});

const KpiSqlSampleSchema = z.object({
  key: z.string(),
  value: z.number(),
  sql: z.string(),
  sample_ids: z.array(z.number()),
});

const DashboardKpiValidationSchema = z.object({
  samples: z.array(KpiSqlSampleSchema),
  overdue_items_total: z.number(),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function getDashboardKpis(): Promise<DashboardKpis> {
  const raw = await invoke<unknown>("get_dashboard_kpis");
  return DashboardKpisSchema.parse(raw);
}

export async function getDashboardWorkloadChart(
  periodDays: number,
): Promise<DashboardWorkloadChart> {
  const raw = await invoke<unknown>("get_dashboard_workload_chart", {
    periodDays,
  });
  return DashboardWorkloadChartSchema.parse(raw);
}

export async function getDashboardLayout(): Promise<DashboardLayoutPayload> {
  const raw = await invoke<unknown>("get_dashboard_layout");
  return DashboardLayoutPayloadSchema.parse(raw);
}

export async function saveDashboardLayout(layoutJson: string): Promise<DashboardLayoutPayload> {
  const raw = await invoke<unknown>("save_dashboard_layout", {
    input: { layout_json: layoutJson },
  });
  return DashboardLayoutPayloadSchema.parse(raw);
}

export async function getDashboardDiStatusChart(): Promise<DashboardDiStatusChart> {
  const raw = await invoke<unknown>("get_dashboard_di_status_chart");
  return DashboardDiStatusChartSchema.parse(raw);
}

export async function getDashboardReliabilitySnapshotSummary(): Promise<DashboardReliabilitySnapshotSummary> {
  const raw = await invoke<unknown>("get_dashboard_reliability_snapshot_summary");
  return DashboardReliabilitySnapshotSummarySchema.parse(raw);
}

export async function getDashboardKpiValidation(): Promise<DashboardKpiValidation> {
  const raw = await invoke<unknown>("get_dashboard_kpi_validation");
  return DashboardKpiValidationSchema.parse(raw);
}

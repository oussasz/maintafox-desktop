// ADR-003: all invoke() calls live exclusively in src/services/.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { DashboardKpis, DashboardWorkloadChart } from "@shared/ipc-types";

// ── Zod schemas ────────────────────────────────────────────────────────────

const KpiValueSchema = z.object({
  key: z.string(),
  value: z.number(),
  previous_value: z.number(),
  available: z.boolean(),
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
});

// ── Service functions ──────────────────────────────────────────────────────

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

// ADR-003: all invoke() calls live exclusively in src/services/.

import { z } from "zod";

import { invoke, invokeSilent } from "@/lib/ipc-invoke";
import type {
  DashboardDiStatusChart,
  DashboardLayoutPayload,
  DashboardWorkloadChart,
  OverviewKpiLayoutPayload,
  OverviewKpisPayload,
  SaveOverviewKpiLayoutInput,
} from "@shared/ipc-types";

// ── Zod schemas ─────────────────────────────────────────────────────────────

/** A series the user may not see comes back `null` (never 0). */
const WorkloadDaySchema = z.object({
  date: z.string(),
  di_created: z.number().nullable(),
  wo_completed: z.number().nullable(),
  pm_due: z.number().nullable(),
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
      avg_age_days: z.number(),
    }),
  ),
  available: z.boolean(),
});

const OverviewKpiCellSchema = z.object({
  semantic_key: z.string(),
  catalog_id: z.string(),
  value: z.number(),
  previous_value: z.number(),
  available: z.boolean(),
  baseline_available: z.boolean().optional(),
  quality_badge: z.string().nullable().optional(),
  value_kind: z.enum(["count", "percent", "hours", "currency"]),
  trend_sense: z.enum(["up-good", "up-bad", "neutral"]),
});

const OverviewKpisPayloadSchema = z.object({
  horizon_days: z.number(),
  cells: z.array(OverviewKpiCellSchema),
});

// ── Service functions ───────────────────────────────────────────────────────

/** Activity over the last `periodDays` days (7 or 30): requests submitted, OTs closed, PM due. */
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

/** Open requests by status (the DI module's open backlog). */
export async function getDashboardDiStatusChart(): Promise<DashboardDiStatusChart> {
  const raw = await invoke<unknown>("get_dashboard_di_status_chart");
  return DashboardDiStatusChartSchema.parse(raw);
}

export async function getOverviewKpis(): Promise<OverviewKpisPayload> {
  const raw = await invokeSilent<unknown>("get_overview_kpis");
  return OverviewKpisPayloadSchema.parse(raw);
}

export async function getOverviewKpiLayout(): Promise<OverviewKpiLayoutPayload> {
  const raw = await invoke<unknown>("get_overview_kpi_layout");
  return z.object({ layout_json: z.string() }).parse(raw);
}

export async function saveOverviewKpiLayout(input: SaveOverviewKpiLayoutInput): Promise<void> {
  await invoke("save_overview_kpi_layout", { input });
}

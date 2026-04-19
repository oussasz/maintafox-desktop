/**
 * di-stats-service.ts
 *
 * IPC wrapper for the DI statistics aggregation command.
 * Phase 2 â€“ Sub-phase 04 â€“ File 04 â€“ Sprint S4.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { DiStatsFilter, DiStatsPayload } from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DiStatusCountSchema = z.object({
  status: z.string(),
  count: z.number(),
});

const DiPriorityCountSchema = z.object({
  priority: z.string(),
  count: z.number(),
});

const DiTypeCountSchema = z.object({
  origin_type: z.string(),
  count: z.number(),
});

const DiTrendPointSchema = z.object({
  period: z.string(),
  created: z.number(),
  closed: z.number(),
});

const DiEquipmentCountSchema = z.object({
  asset_id: z.number(),
  asset_label: z.string(),
  count: z.number(),
  percentage: z.number(),
});

const DiOverdueDiSchema = z.object({
  id: z.number(),
  code: z.string(),
  title: z.string(),
  priority: z.string(),
  days_overdue: z.number(),
});

const DiStatsPayloadSchema = z.object({
  total: z.number(),
  pending: z.number(),
  in_progress: z.number(),
  closed: z.number(),
  closed_this_month: z.number(),
  overdue: z.number(),
  sla_met_count: z.number(),
  sla_total: z.number(),
  safety_issues: z.number(),
  status_distribution: z.array(DiStatusCountSchema),
  priority_distribution: z.array(DiPriorityCountSchema),
  type_distribution: z.array(DiTypeCountSchema),
  monthly_trend: z.array(DiTrendPointSchema),
  available_years: z.array(z.number()),
  avg_age_days: z.number(),
  max_age_days: z.number(),
  top_equipment: z.array(DiEquipmentCountSchema),
  overdue_dis: z.array(DiOverdueDiSchema),
});

// â”€â”€ Command â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function getDiStats(filter: DiStatsFilter): Promise<DiStatsPayload> {
  const raw = await invoke<unknown>("get_di_stats", { filter });
  return DiStatsPayloadSchema.parse(raw) as DiStatsPayload;
}

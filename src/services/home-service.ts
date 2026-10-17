// ADR-003: all invoke() calls live exclusively in src/services/.
//
// Home page (start centre): one aggregated call per role panel (`commands::home`). Every figure
// is the owning module's definition; a `null` section means the user may not open that module.
// A malformed answer is refused (the panel shows its error with a retry), never shown as 0.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { DiQueueCountsSchema } from "@/services/di-service";
import type { HomeMyWork, HomePlanning, HomeSupervision } from "@shared/ipc-types";

/** Periods offered by the panels' period selector (days) — `home::PERIOD_CHOICES`. */
export const HOME_PERIODS = [7, 30] as const;
export type HomePeriod = (typeof HOME_PERIODS)[number];

export function isHomePeriod(value: number): value is HomePeriod {
  return (HOME_PERIODS as readonly number[]).includes(value);
}

const nullableString = z.string().nullable();

const MyWorkSchema = z.object({
  as_of: z.string(),
  work_orders: z
    .object({
      open: z.number(),
      overdue: z.number(),
      today: z.number(),
      by_urgency: z.array(z.object({ urgency_code: nullableString, count: z.number() })),
      items: z.array(
        z.object({
          id: z.number(),
          code: z.string(),
          title: z.string(),
          status_code: z.string(),
          urgency_code: nullableString,
          planned_start: nullableString,
          planned_end: nullableString,
          asset_label: nullableString,
          overdue: z.boolean(),
        }),
      ),
    })
    .nullable(),
  requests: z.object({ open: z.number(), awaiting_my_answer: z.number() }).nullable(),
  rounds: z
    .object({
      personnel_linked: z.boolean(),
      overdue: z.number(),
      today: z.number(),
      next_7_days: z.number(),
      todo: z.number(),
      items: z.array(
        z.object({
          id: z.number(),
          template_code: z.string(),
          template_name: z.string(),
          status: z.string(),
          scheduled_at: nullableString,
          overdue: z.boolean(),
        }),
      ),
    })
    .nullable(),
  pm: z
    .object({
      overdue: z.number(),
      due_next_7_days: z.number(),
      pending: z.number(),
      items: z.array(
        z.object({
          id: z.number(),
          plan_code: z.string(),
          plan_title: z.string(),
          due_at: nullableString,
          work_order_code: z.string(),
          overdue: z.boolean(),
        }),
      ),
    })
    .nullable(),
  notifications: z.object({ unread: z.number(), to_acknowledge: z.number() }),
});

const PlanningSchema = z.object({
  as_of: z.string(),
  backlog: z.object({
    candidates: z.number(),
    ready: z.number(),
    blocked: z.number(),
    blocked_by: z.array(z.object({ dimension: z.string(), count: z.number() })),
    truncated: z.boolean(),
  }),
  week: z
    .object({
      week_start: z.string(),
      week_end: z.string(),
      sites: z.number(),
      sites_not_frozen: z.number(),
      baseline_hours: z.number(),
      available_hours: z.number(),
      overtime_hours: z.number(),
      load_pct: z.number().nullable(),
      relies_on_overtime: z.boolean(),
      weeks_awaiting_approval: z.number(),
    })
    .nullable(),
  week_unavailable: z.enum(["WEEK_NO_PLANNING_SITE", "WEEK_NOT_FROZEN"]).nullable(),
  schedule_compliance: z.object({
    period_days: z.number(),
    scheduled_hours: z.number(),
    done_hours: z.number(),
    released_jobs: z.number(),
    value_pct: z.number().nullable(),
    unavailable: z.enum(["NO_FROZEN_BASELINE"]).nullable(),
  }),
});

const PermitItemSchema = z.object({
  permit_id: z.number(),
  code: z.string(),
  permit_type: z.string(),
  asset_code: z.string(),
  expires_at: z.string(),
});

const PermitListSchema = z.object({
  total: z.number(),
  items: z.array(PermitItemSchema),
  without_record: z.number(),
});

const SupervisionSchema = z.object({
  as_of: z.string(),
  requests: z
    .object({
      sla_open: z.object({
        on_track: z.number(),
        at_risk: z.number(),
        breached: z.number(),
        paused: z.number(),
        no_target: z.number(),
      }),
      period_days: z.number(),
      sla_closing: z.object({ met: z.number(), breached: z.number(), total: z.number() }),
      queues: DiQueueCountsSchema,
    })
    .nullable(),
  work_orders: z
    .object({
      open: z.number(),
      overdue: z.number(),
      planned: z.number(),
      reactive: z.number(),
      emergency: z.number(),
      without_nature: z.number(),
      cells: z.array(
        z.object({ nature_code: nullableString, urgency_code: nullableString, count: z.number() }),
      ),
    })
    .nullable(),
  pm: z
    .object({
      overdue: z.number(),
      due_today: z.number(),
      due_next_7_days: z.number(),
      period_days: z.number(),
      compliance_done_on_time: z.number(),
      compliance_due: z.number(),
      compliance_pct: z.number().nullable(),
    })
    .nullable(),
  stock: z
    .object({
      lines_to_reorder: z.number(),
      articles_to_reorder: z.number(),
      lines_out_of_stock: z.number(),
      items: z.array(
        z.object({
          article_id: z.number(),
          article_code: z.string(),
          article_name: z.string(),
          warehouse_code: z.string(),
          available_qty: z.number(),
          reorder_point: z.number(),
          min_stock: z.number(),
          trigger_type: z.string(),
        }),
      ),
    })
    .nullable(),
  safety: z
    .object({
      open_permits: z.number(),
      expiring_rule: z.enum(["active", "disabled", "no_lead", "missing"]),
      expiring_lead_minutes: z.number().nullable(),
      expiring: PermitListSchema,
      expired_open: PermitListSchema,
      locks_in_place: z.number(),
      permits_with_locks: z.number(),
    })
    .nullable(),
  assets: z
    .object({
      critical_down: z.number(),
      criticality_label: nullableString,
      down_status_labels: z.array(z.string()),
      items: z.array(
        z.object({
          id: z.number(),
          asset_code: z.string(),
          name: z.string(),
          status_label: nullableString,
        }),
      ),
    })
    .nullable(),
});

/** "My work": the signed-in user's work orders, requests, rounds, PM and notifications. */
export async function getHomeMyWork(): Promise<HomeMyWork> {
  const raw = await invoke<unknown>("get_home_my_work");
  return MyWorkSchema.parse(raw);
}

/** Planner panel; schedule compliance over `periodDays` (7 or 30). */
export async function getHomePlanning(periodDays: HomePeriod): Promise<HomePlanning> {
  const raw = await invoke<unknown>("get_home_planning", { periodDays });
  return PlanningSchema.parse(raw);
}

/** Supervisor / manager panel; SLA and PM compliance over `periodDays` (7 or 30). */
export async function getHomeSupervision(periodDays: HomePeriod): Promise<HomeSupervision> {
  const raw = await invoke<unknown>("get_home_supervision", { periodDays });
  return SupervisionSchema.parse(raw);
}

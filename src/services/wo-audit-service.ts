/**
 * wo-audit-service.ts
 *
 * IPC wrappers for WO audit change-event queries.
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S3.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// ── Zod schemas ───────────────────────────────────────────────────────────────

const WoChangeEventSchema = z.object({
  id: z.number(),
  wo_id: z.number().nullable(),
  action: z.string(),
  actor_id: z.number().nullable(),
  acted_at: z.string(),
  summary: z.string().nullable(),
  details_json: z.string().nullable(),
  requires_step_up: z.number(),
  apply_result: z.string(),
});

// ── Exported types ────────────────────────────────────────────────────────────

export type WoChangeEvent = z.infer<typeof WoChangeEventSchema>;

export interface WoAuditFilter {
  action?: string;
  actorId?: number;
  dateFrom?: string;
  dateTo?: string;
  woId?: number;
  limit?: number;
  offset?: number;
}

// ── Commands ──────────────────────────────────────────────────────────────────

/**
 * List audit change events for a single WO, ordered chronologically.
 * Permission: ot.view
 */
export async function listWoChangeEvents(woId: number, limit?: number): Promise<WoChangeEvent[]> {
  const raw = await invoke<unknown>("list_wo_change_events", {
    woId,
    limit: limit ?? null,
  });
  return z.array(WoChangeEventSchema).parse(raw);
}

/**
 * List all WO audit change events matching a filter (admin view).
 * Permission: ot.admin
 */
export async function listAllWoChangeEvents(filter: WoAuditFilter): Promise<WoChangeEvent[]> {
  const raw = await invoke<unknown>("list_all_wo_change_events", {
    filter: {
      action: filter.action ?? null,
      actor_id: filter.actorId ?? null,
      date_from: filter.dateFrom ?? null,
      date_to: filter.dateTo ?? null,
      wo_id: filter.woId ?? null,
      limit: filter.limit ?? null,
      offset: filter.offset ?? null,
    },
  });
  return z.array(WoChangeEventSchema).parse(raw);
}

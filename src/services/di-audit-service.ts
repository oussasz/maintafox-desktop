/**
 * di-audit-service.ts
 *
 * IPC wrappers for DI audit change-event queries.
 * Phase 2 â€“ Sub-phase 04 â€“ File 04 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DiChangeEventSchema = z.object({
  id: z.number(),
  di_id: z.number().nullable(),
  action: z.string(),
  actor_id: z.number().nullable(),
  acted_at: z.string(),
  summary: z.string().nullable(),
  details_json: z.string().nullable(),
  requires_step_up: z.number(),
  apply_result: z.string(),
});

// â”€â”€ Exported types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export type DiChangeEvent = z.infer<typeof DiChangeEventSchema>;

export interface DiAuditFilter {
  action?: string;
  actorId?: number;
  dateFrom?: string;
  dateTo?: string;
  limit?: number;
  offset?: number;
}

// â”€â”€ Commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * List audit change events for a single DI, ordered most-recent first.
 */
export async function listDiChangeEvents(diId: number, limit?: number): Promise<DiChangeEvent[]> {
  const raw = await invoke<unknown>("list_di_change_events", {
    diId,
    limit: limit ?? null,
  });
  return z.array(DiChangeEventSchema).parse(raw) as DiChangeEvent[];
}

/**
 * List all DI audit change events matching a filter (admin view).
 */
export async function listAllDiChangeEvents(filter: DiAuditFilter): Promise<DiChangeEvent[]> {
  const raw = await invoke<unknown>("list_all_di_change_events", {
    filter: {
      action: filter.action ?? null,
      actor_id: filter.actorId ?? null,
      date_from: filter.dateFrom ?? null,
      date_to: filter.dateTo ?? null,
      limit: filter.limit ?? null,
      offset: filter.offset ?? null,
    },
  });
  return z.array(DiChangeEventSchema).parse(raw) as DiChangeEvent[];
}

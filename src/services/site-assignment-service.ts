/**
 * "Records without a site" (CMMS Advisor decision Q1, step 6 site-scoped roles).
 *
 * Old work orders, requests and planning windows recorded before a site was required may have
 * none. A plant-wide organisation administrator (`org.admin` for the whole plant) gives them a
 * site here: only records without one, a reason for the batch, one audit entry per record
 * (`org::site_assignment` in the backend, which checks everything again).
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { AssignableScopeNodeSchema } from "@/services/rbac-service";
import type {
  AssignSiteInput,
  AssignSiteResult,
  NoSiteCounts,
  NoSiteKind,
  RecordsWithoutSite,
} from "@shared/ipc-types";

export const NoSiteKindSchema = z.enum(["work_order", "request", "planning_window"]);

export const NoSiteCountsSchema = z.object({
  work_orders: z.number(),
  requests: z.number(),
  planning_windows_to_review: z.number(),
  planning_windows_plant_wide: z.number(),
});

export const NoSiteRecordSchema = z.object({
  kind: NoSiteKindSchema,
  id: z.number(),
  code: z.string().nullable(),
  title: z.string().nullable(),
  status_code: z.string().nullable(),
  created_at: z.string().nullable(),
  window_start: z.string().nullable(),
  window_end: z.string().nullable(),
  equipment_id: z.number().nullable(),
  equipment_code: z.string().nullable(),
  equipment_name: z.string().nullable(),
  suggested_node_id: z.number().nullable(),
  suggested_node_name: z.string().nullable(),
  to_review: z.boolean(),
});

export const RecordsWithoutSiteSchema = z.object({
  kind: NoSiteKindSchema,
  counts: NoSiteCountsSchema,
  records: z.array(NoSiteRecordSchema),
  listed_limit: z.number(),
  max_batch: z.number(),
  reason_min_chars: z.number(),
  reason_max_chars: z.number(),
  nodes: z.array(AssignableScopeNodeSchema),
});

export const AssignSiteResultSchema = z.object({
  kind: NoSiteKindSchema,
  org_node_id: z.number(),
  assigned: z.array(z.number()),
  counts: NoSiteCountsSchema,
});

/** The records "to review: no site" (plant-wide windows are valid and not counted). */
export function noSiteToReview(counts: NoSiteCounts): number {
  return counts.work_orders + counts.requests + counts.planning_windows_to_review;
}

/** How many records have no site, per kind (plant-wide `org.admin`). */
export async function countRecordsWithoutSite(): Promise<NoSiteCounts> {
  const raw = await invoke<unknown>("count_records_without_site");
  return NoSiteCountsSchema.parse(raw);
}

/** The records of one kind without a site, the counts and the nodes a site is chosen from. */
export async function listRecordsWithoutSite(kind: NoSiteKind): Promise<RecordsWithoutSite> {
  const raw = await invoke<unknown>("list_records_without_site", { kind });
  return RecordsWithoutSiteSchema.parse(raw);
}

/** Gives a site to records without one (step-up; the backend refuses a record that has a site). */
export async function assignSiteToRecords(payload: AssignSiteInput): Promise<AssignSiteResult> {
  const raw = await invoke<unknown>("assign_site_to_records", { payload });
  return AssignSiteResultSchema.parse(raw);
}

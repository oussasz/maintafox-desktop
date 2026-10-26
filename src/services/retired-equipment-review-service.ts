/**
 * Work orders and requests open on retired equipment, crossed in sync: "équipement déclassé — à
 * revoir" (backend `wo::retired_equipment_review`, audit finding AU-73b).
 *
 * A work order or request created on one PC while its equipment was in service, and the
 * retirement recorded on another, meet at sync: the received row is applied (a field fact) and
 * raises one review item per work order or request, keyed by its sync id (the same on every PC).
 * The item closes on any terminal state (work order closed or cancelled; request rejected, closed
 * or converted) or when the equipment is returned to service, and keeps who, when and how. It is
 * settled only with the existing actions: there is nothing to call here but the list.
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

export const RETIRED_REVIEW_CLOSURES = [
  "wo_closed",
  "wo_cancelled",
  "di_rejected",
  "di_closed",
  "di_converted",
  "recommissioned",
  "other",
] as const;

export const RetiredEquipmentReviewSchema = z.object({
  id: z.number(),
  review_key: z.string(),
  kind: z.enum(["work_order", "intervention_request"]),
  record_id: z.number().nullable(),
  record_code: z.string().nullable(),
  record_title: z.string().nullable(),
  record_status: z.string().nullable(),
  equipment_id: z.number(),
  equipment_code: z.string().nullable(),
  equipment_name: z.string().nullable(),
  equipment_status: z.string().nullable(),
  status: z.enum(["open", "closed"]),
  opened_at: z.string(),
  closed_at: z.string().nullable(),
  closure_kind: z.enum(RETIRED_REVIEW_CLOSURES).nullable(),
  closed_by: z.string().nullable(),
  closure_note: z.string().nullable(),
  converted_to_wo_id: z.number().nullable(),
  converted_to_wo_code: z.string().nullable(),
  disposition_code: z.string().nullable(),
});
export type RetiredEquipmentReview = z.infer<typeof RetiredEquipmentReviewSchema>;

export const RetiredEquipmentReviewListSchema = z.object({
  items: z.array(RetiredEquipmentReviewSchema),
  /** Every closed item the reader may see (the history lists the latest 100). */
  closed_count: z.number(),
});
export type RetiredEquipmentReviewList = z.infer<typeof RetiredEquipmentReviewListSchema>;

/**
 * Open items, then (when asked: the history) the latest closed ones; ot.view, requests only where
 * di.view.
 */
export async function listRetiredEquipmentReviews(
  includeClosed: boolean,
): Promise<RetiredEquipmentReviewList> {
  const raw = await invoke<unknown>("list_retired_equipment_reviews", { includeClosed });
  return RetiredEquipmentReviewListSchema.parse(raw);
}

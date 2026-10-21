/**
 * Stock balances below zero after sync, to reconcile (backend `inventory::negative_stock`).
 *
 * A movement received from another PC is applied as received; a location left below zero gets one
 * review item per article and location, closed by a stock count recorded there (inv.manage, a
 * reason) or by a later stock entry. Two counts of the same location recorded on different PCs
 * without knowing each other are a double count to verify, closed by a new count. More reserved
 * than held is a warning only.
 *
 * IPC failures are passed on untouched so their coded reasons (`inventory:errors.*`) reach the
 * screen (`appErrorReason`).
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Shortest reason accepted for the count (characters, trimmed): `REVIEW_REASON_MIN_CHARS`. */
export const NEGATIVE_STOCK_REASON_MIN_CHARS = 8;

export const NegativeStockReviewSchema = z.object({
  id: z.number(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  status: z.enum(["open", "closed"]),
  opened_at: z.string(),
  opening_transaction_id: z.number().nullable(),
  opening_movement_type: z.string().nullable(),
  closed_at: z.string().nullable(),
  closure_kind: z
    .enum(["count", "adjustment", "receipt", "return", "prior_entry", "other"])
    .nullable(),
  closing_transaction_id: z.number().nullable(),
  closing_movement_type: z.string().nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
});
export type NegativeStockReview = z.infer<typeof NegativeStockReviewSchema>;

export const OverReservationWarningSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
});
export type OverReservationWarning = z.infer<typeof OverReservationWarningSchema>;

export const ReconciliationCountSchema = z.object({
  /** Sync id of the count line. */
  count_sync_id: z.string(),
  session_code: z.string(),
  /** Which path posted the count: the stock-to-reconcile list or a count session. */
  path: z.enum(["reconciliation", "count_session"]),
  counted_by: z.string().nullable(),
  device_code: z.string(),
  counted_at: z.string(),
  /** Stock on record the count was set against, on the PC that posted it. */
  system_qty: z.number(),
  counted_qty: z.number(),
  variance_qty: z.number(),
});
export type ReconciliationCount = z.infer<typeof ReconciliationCountSchema>;

/** The reversal of a count session that ended a double count. */
export const CountReversalSchema = z.object({
  count_sync_id: z.string(),
  session_code: z.string(),
  reversed_by: z.string().nullable(),
  reversed_at: z.string(),
});
export type CountReversal = z.infer<typeof CountReversalSchema>;

export const DoubleCountReviewSchema = z.object({
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  counts: z.array(ReconciliationCountSchema),
  /** The reversal of one of its counts that closed it (null while it is to verify). */
  closed_by_reversal: CountReversalSchema.nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
});
export type DoubleCountReview = z.infer<typeof DoubleCountReviewSchema>;

const NegativeStockReviewListSchema = z.object({
  items: z.array(NegativeStockReviewSchema),
  double_counts: z.array(DoubleCountReviewSchema),
  /** Double counts closed by the reversal of one of their counts (when closed items are asked). */
  closed_double_counts: z.array(DoubleCountReviewSchema),
  over_reservations: z.array(OverReservationWarningSchema),
});
export type NegativeStockReviewList = z.infer<typeof NegativeStockReviewListSchema>;

const NegativeStockSummarySchema = z.object({
  negative_count: z.number(),
  double_count_count: z.number(),
  over_reserved_count: z.number(),
});
export type NegativeStockSummary = z.infer<typeof NegativeStockSummarySchema>;

const NegativeStockResolutionSchema = z.object({
  session_code: z.string(),
  counted_qty: z.number(),
  variance_qty: z.number(),
  review: NegativeStockReviewSchema.nullable(),
});
export type NegativeStockResolution = z.infer<typeof NegativeStockResolutionSchema>;

export interface ResolveNegativeStockInput {
  article_id: number;
  location_id: number;
  /** Quantity physically counted at the location (zero or more). */
  counted_qty: number;
  /** Code of the plant's variance-reason list (`inventory.variance_reason`). */
  variance_reason_code: string;
  reason: string;
}

export async function listNegativeStockReviews(
  includeClosed: boolean,
): Promise<NegativeStockReviewList> {
  const raw = await invoke<unknown>("list_negative_stock_reviews", { includeClosed });
  return NegativeStockReviewListSchema.parse(raw);
}

export async function getNegativeStockSummary(): Promise<NegativeStockSummary> {
  const raw = await invoke<unknown>("get_negative_stock_summary");
  return NegativeStockSummarySchema.parse(raw);
}

export async function resolveNegativeStockByCount(
  input: ResolveNegativeStockInput,
): Promise<NegativeStockResolution> {
  const raw = await invoke<unknown>("resolve_negative_stock_by_count", { input });
  return NegativeStockResolutionSchema.parse(raw);
}

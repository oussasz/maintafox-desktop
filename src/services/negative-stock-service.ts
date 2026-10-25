/**
 * Stock balances below zero after sync, to reconcile (backend `inventory::negative_stock`).
 *
 * A movement received from another PC is applied as received; a location left below zero gets one
 * review item per article and location, closed by a stock count recorded there (inv.manage, a
 * reason) or by a later stock entry. Two counts of the same location recorded on different PCs
 * without knowing each other are a double count to verify, closed by a new count; so is a count
 * whose movement is in the ledger twice (posted on two PCs by older versions). A reservation
 * issued, or a PO line received, beyond its quantity by movements of several PCs is a
 * "sur-sortie / sur-réception — à rapprocher", closed the same way. More reserved than held is a
 * warning only.
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
  /** Who posted it (the session's poster; the person counting on the list). */
  counted_by: z.string().nullable(),
  /** Who approved its variance, when it had one and the account is known on this PC. */
  approved_by: z.string().nullable(),
  /** Auto-approbation, derived on every PC: flagged, or its variance approved by its own counter. */
  self_approved: z.boolean(),
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

/** One movement of a count posted twice. */
export const DoublePostedMovementSchema = z.object({
  movement_sync_id: z.string(),
  posted_by: z.string().nullable(),
  posted_at: z.string(),
  quantity: z.number(),
  /** The PC that posted it, when known (the audit of a divergent count names it). */
  device_code: z.string().nullable(),
});
export type DoublePostedMovement = z.infer<typeof DoublePostedMovementSchema>;

/**
 * "Comptage passé deux fois — à vérifier": one count of a session, article and location whose
 * movement is in the stock ledger more than once (the same count posted, or reversed, on two PCs
 * by older versions). Closed only by a new count of that article and location.
 */
export const DoublePostedCountReviewSchema = z.object({
  /** `posted_twice`: two movements in the ledger; `divergent`: posted on two PCs with different quantities. */
  kind: z.enum(["posted_twice", "divergent"]),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  session_code: z.string(),
  movement_type: z.enum(["COUNT_ADJUST", "COUNT_REVERSAL"]),
  movements: z.array(DoublePostedMovementSchema),
  /** The count's correction (one movement). */
  variance_qty: z.number(),
  /** What the stock moved beyond that correction. */
  excess_qty: z.number(),
  /** The count that closed it (null while it is to verify). */
  closed_by: ReconciliationCountSchema.nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
});
export type DoublePostedCountReview = z.infer<typeof DoublePostedCountReviewSchema>;

/** One movement of an over-issue (issue or return of the reservation) or over-receipt (receipt entry). */
export const OverQuantityMovementSchema = z.object({
  movement_sync_id: z.string(),
  /**
   * `ISSUE`, `RETURN` or `GR_ACCEPT` today. Kept a string so a movement type added later shows
   * under its code instead of making the whole list unreadable.
   */
  movement_type: z.string(),
  /** The document it was recorded on (goods-receipt number), when any. */
  document_ref: z.string().nullable(),
  posted_by: z.string().nullable(),
  posted_at: z.string(),
  quantity: z.number(),
});
export type OverQuantityMovement = z.infer<typeof OverQuantityMovementSchema>;

/**
 * "Sur-sortie / sur-réception — à rapprocher": one reservation issued beyond its reserved
 * quantity, or one PO line received beyond its ordered quantity, by movements of several PCs
 * (listed per location of the receipts). Closed by a count of that article and location made on a
 * PC holding every one of its movements (from this list, a count session or an inventory campaign)
 * or, for an over-issue, by a return of the reservation back to its reserved quantity
 * ("régularisé par retour").
 */
export const OverQuantityReviewSchema = z.object({
  kind: z.enum(["over_issue", "over_receipt"]),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  /** Over-issue: the work order of the reservation, when it has one. */
  work_order_code: z.string().nullable(),
  /** Over-issue: the reservation's own reference. */
  reservation_ref: z.string().nullable(),
  /** Over-receipt: the purchase order of the line. */
  purchase_order_number: z.string().nullable(),
  /** Reserved (over-issue) or ordered (over-receipt). */
  expected_qty: z.number(),
  /** Issued net of returns (at its highest when a return closed it), or received on every receipt. */
  actual_qty: z.number(),
  excess_qty: z.number(),
  movements: z.array(OverQuantityMovementSchema),
  /** Null while it is to reconcile; closed by a return of the reservation or by a count. */
  closure_kind: z.enum(["return", "count"]).nullable(),
  /** The return that closed it (who, when, the quantity returned). */
  closed_by_return: OverQuantityMovementSchema.nullable(),
  /** The count that closed it: its session, who counted and who approved. */
  closed_by: ReconciliationCountSchema.nullable(),
  on_hand_qty: z.number(),
  reserved_qty: z.number(),
  available_qty: z.number(),
});
export type OverQuantityReview = z.infer<typeof OverQuantityReviewSchema>;

/**
 * A count recorded from this list above the four-eyes tolerance, waiting for a second person's
 * approval before it moves the stock ("comptage en attente d'approbation").
 */
export const PendingReconciliationCountSchema = z.object({
  line_id: z.number(),
  row_version: z.number(),
  session_code: z.string(),
  article_id: z.number(),
  article_code: z.string(),
  article_name: z.string(),
  location_id: z.number(),
  location_code: z.string(),
  warehouse_code: z.string(),
  counted_by_id: z.number().nullable(),
  counted_by: z.string().nullable(),
  counted_at: z.string(),
  system_qty: z.number(),
  counted_qty: z.number(),
  variance_qty: z.number(),
  variance_reason_code: z.string().nullable(),
});
export type PendingReconciliationCount = z.infer<typeof PendingReconciliationCountSchema>;

const NegativeStockReviewListSchema = z.object({
  items: z.array(NegativeStockReviewSchema),
  double_counts: z.array(DoubleCountReviewSchema),
  /** Double counts closed by the reversal of one of their counts (when closed items are asked). */
  closed_double_counts: z.array(DoubleCountReviewSchema),
  double_postings: z.array(DoublePostedCountReviewSchema),
  /** Counts posted twice that a new count closed (when closed items are asked). */
  closed_double_postings: z.array(DoublePostedCountReviewSchema),
  over_quantities: z.array(OverQuantityReviewSchema),
  /** Over-issues and over-receipts that a return or a count closed (when closed items are asked). */
  closed_over_quantities: z.array(OverQuantityReviewSchema),
  over_reservations: z.array(OverReservationWarningSchema),
  /** Counts above the four-eyes tolerance waiting for a second person's approval. */
  pending_counts: z.array(PendingReconciliationCountSchema),
  /** The stores' site opted out of four-eyes: the counter may approve a waiting count. */
  self_approval_allowed: z.boolean(),
});
export type NegativeStockReviewList = z.infer<typeof NegativeStockReviewListSchema>;

const NegativeStockSummarySchema = z.object({
  negative_count: z.number(),
  double_count_count: z.number(),
  /** Counts posted twice (same count on two PCs, older versions) still to verify. */
  double_posting_count: z.number(),
  /** Over-issues and over-receipts (several PCs) still to reconcile. */
  over_quantity_count: z.number(),
  over_reserved_count: z.number(),
});
export type NegativeStockSummary = z.infer<typeof NegativeStockSummarySchema>;

const NegativeStockResolutionSchema = z.object({
  session_code: z.string(),
  counted_qty: z.number(),
  variance_qty: z.number(),
  review: NegativeStockReviewSchema.nullable(),
  /** Above the four-eyes tolerance: saved as counted, waiting for a second person's approval. */
  pending_approval: z.boolean(),
  /** Approved by the person counting under the site's opt-out ("auto-approbation"). */
  self_approved: z.boolean(),
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

export interface ApprovePendingCountInput {
  line_id: number;
  expected_row_version: number;
  /** The approver's note (at least `NEGATIVE_STOCK_REASON_MIN_CHARS` characters). */
  reason: string;
}

/** Approves (and posts) a count waiting for its approval; the approver is the signed-in user. */
export async function approvePendingNegativeStockCount(
  input: ApprovePendingCountInput,
): Promise<NegativeStockResolution> {
  const raw = await invoke<unknown>("approve_negative_stock_count", { input });
  return NegativeStockResolutionSchema.parse(raw);
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

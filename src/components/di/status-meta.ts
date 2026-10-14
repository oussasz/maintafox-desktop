import { DI_STATUS_TONE, DI_URGENCY_TONE, toneChipMap } from "@/design-system/tone";

export type DiStatusKey =
  | "new"
  | "pendingReview"
  | "awaitingApproval"
  | "needsClarification"
  | "inReview"
  | "approved"
  | "deferred"
  | "closed"
  | "cancelled";

export const DI_STATUS_STYLE: Record<string, string> = toneChipMap(DI_STATUS_TONE);

/** Chip classes per DI urgency (low / medium / high / critical). */
export const DI_URGENCY_STYLE: Record<string, string> = toneChipMap(DI_URGENCY_TONE);

export const TERMINAL_DI_STATES = new Set(["closed"]);

export function diStatusToI18nKey(status: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    none: "new",
    submitted: "new",
    in_review: "inReview",
    pending_review: "inReview",
    returned_for_clarification: "needsClarification",
    awaiting_approval: "awaitingApproval",
    screened: "awaitingApproval",
    approved: "approved",
    approved_for_planning: "approved",
    deferred: "deferred",
    closed: "closed",
    rejected: "closed",
    converted_to_work_order: "closed",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[status] ?? "new";
}

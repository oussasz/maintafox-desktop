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

export const DI_STATUS_STYLE: Record<string, string> = {
  submitted: "bg-blue-100 text-blue-800",
  in_review: "bg-sky-100 text-sky-800",
  returned_for_clarification: "bg-orange-100 text-orange-800",
  awaiting_approval: "bg-indigo-100 text-indigo-800",
  approved: "bg-green-100 text-green-800",
  deferred: "bg-gray-100 text-gray-600",
  closed: "bg-slate-100 text-slate-700",
};

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

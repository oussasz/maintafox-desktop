export type DiStatusKey =
  | "new"
  | "pendingReview"
  | "awaitingApproval"
  | "needsClarification"
  | "inReview"
  | "approved"
  | "rejected"
  | "inProgress"
  | "resolved"
  | "closed"
  | "cancelled";

export const DI_STATUS_STYLE: Record<string, string> = {
  submitted: "bg-blue-100 text-blue-800",
  pending_review: "bg-sky-100 text-sky-800",
  returned_for_clarification: "bg-orange-100 text-orange-800",
  rejected: "bg-red-100 text-red-700",
  screened: "bg-indigo-100 text-indigo-800",
  awaiting_approval: "bg-indigo-100 text-indigo-800",
  approved_for_planning: "bg-green-100 text-green-800",
  deferred: "bg-gray-100 text-gray-600",
  converted_to_work_order: "bg-emerald-100 text-emerald-800",
  closed_as_non_executable: "bg-slate-100 text-slate-600",
  archived: "bg-neutral-100 text-neutral-500",
};

export const TERMINAL_DI_STATES = new Set([
  "rejected",
  "converted_to_work_order",
  "archived",
]);

export function diStatusToI18nKey(status: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    none: "new",
    submitted: "new",
    pending_review: "pendingReview",
    returned_for_clarification: "needsClarification",
    rejected: "rejected",
    screened: "awaitingApproval",
    awaiting_approval: "awaitingApproval",
    approved_for_planning: "approved",
    deferred: "inReview",
    converted_to_work_order: "inProgress",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[status] ?? "new";
}

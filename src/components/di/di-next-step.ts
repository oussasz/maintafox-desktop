/**
 * What happens next to a request, in the requester's words: the translation key under
 * `di:nextStep.*` and its parameters. Derived only from the request's own state.
 */

import type { InterventionRequest } from "@shared/ipc-types";

export type DiNextStepKey =
  | "nextStep.screening"
  | "nextStep.review"
  | "nextStep.answer"
  | "nextStep.approval"
  | "nextStep.workOrder"
  | "nextStep.deferred"
  | "nextStep.converted"
  | "nextStep.closed";

export interface DiNextStep {
  key: DiNextStepKey;
  /** The requester is the one who must act (answer the reviewer). */
  requesterAction: boolean;
  /** Deferral date (ISO) for `nextStep.deferred`. */
  until?: string | null | undefined;
  /** Work order code for `nextStep.converted`. */
  woCode?: string | null | undefined;
}

export function diNextStep(di: InterventionRequest): DiNextStep {
  if (di.converted_to_wo_id != null) {
    return { key: "nextStep.converted", requesterAction: false, woCode: di.converted_to_wo_code };
  }
  switch (di.status) {
    case "submitted":
      return { key: "nextStep.screening", requesterAction: false };
    case "in_review":
      return { key: "nextStep.review", requesterAction: false };
    case "returned_for_clarification":
      return { key: "nextStep.answer", requesterAction: true };
    case "awaiting_approval":
      return { key: "nextStep.approval", requesterAction: false };
    case "approved":
      return { key: "nextStep.workOrder", requesterAction: false };
    case "deferred":
      return { key: "nextStep.deferred", requesterAction: false, until: di.deferred_until };
    default:
      return { key: "nextStep.closed", requesterAction: false };
  }
}

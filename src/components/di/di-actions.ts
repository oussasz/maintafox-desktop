/**
 * Which decisions the signed-in user may take on a request, from its status and their
 * permissions. Mirrors the backend gates (`commands/di.rs`) so no button is shown that the
 * backend would refuse; the backend stays the authority.
 *
 * `can` answers at the request's site (`useAccessAt(di.org_node_id)`): a right held on another
 * site only does not offer a decision on this request.
 */

import type { InterventionRequest } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

type Can = (permission: PermissionName) => boolean;

export interface DiActions {
  /** Edit the request fields (submitted or returned for information). */
  edit: boolean;
  /** Admit a submitted request into the review queue. */
  triage: boolean;
  /** Screening decision (urgency, classification) of a request in review. */
  screen: boolean;
  /** Send back to the requester for information. */
  returnForInfo: boolean;
  /** The requester answers and resubmits. */
  resubmit: boolean;
  approve: boolean;
  convert: boolean;
  defer: boolean;
  reactivate: boolean;
  /** Close with a disposition (reject, duplicate, no work required…). */
  close: boolean;
  cancelOwn: boolean;
  archive: boolean;
}

export function diActions(
  di: InterventionRequest,
  userId: number | null | undefined,
  can: Can,
): DiActions {
  const isOwner = userId != null && userId === di.submitter_id;
  const screener = can(P.DI_SCREEN) || can(P.DI_REVIEW);
  const editable = di.status === "submitted" || di.status === "returned_for_clarification";
  return {
    edit: editable && ((isOwner && can(P.DI_CREATE_OWN)) || can(P.DI_REVIEW)),
    triage: di.status === "submitted" && screener,
    screen: di.status === "in_review" && screener,
    returnForInfo: di.status === "in_review" && can(P.DI_REVIEW),
    resubmit: di.status === "returned_for_clarification" && isOwner && can(P.DI_CREATE_OWN),
    approve: di.status === "awaiting_approval" && can(P.DI_APPROVE),
    // The work order is created where the request is: the work-order create right is needed too.
    convert:
      di.status === "approved" &&
      di.converted_to_wo_id == null &&
      can(P.DI_CONVERT) &&
      can(P.OT_CREATE),
    defer:
      (di.status === "in_review" ||
        di.status === "awaiting_approval" ||
        di.status === "approved") &&
      can(P.DI_APPROVE),
    reactivate: di.status === "deferred" && can(P.DI_APPROVE),
    close:
      (di.status === "in_review" && can(P.DI_REVIEW)) ||
      ((di.status === "awaiting_approval" || di.status === "approved") && can(P.DI_APPROVE)),
    cancelOwn: editable && isOwner && can(P.DI_CREATE_OWN),
    archive: di.status === "closed" && di.archived_at == null && can(P.DI_APPROVE),
  };
}

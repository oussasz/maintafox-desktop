/**
 * Site scope of the PM plan screens (site-scoped roles; backend `pm::access`).
 *
 * A plan's rights (`pm.create` for a new version or a revision, `pm.edit`, `pm.approve`,
 * `pm.delete`) are checked at its org node: its equipment's site, or its location. A family or
 * criticality-group plan (and a plan on equipment installed nowhere) has none: plant-wide rights
 * only, read at node `0` as for a work order without a site. The backend checks the same and stays
 * the authority.
 *
 * On screen (L6-08 rule): an action whose right the user holds nowhere is hidden; one held only on
 * other sites is shown disabled with the reason.
 */

import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { type AccessAt, useAccessAt } from "@/hooks/use-access-at";
import { usePermissions } from "@/hooks/use-permissions";
import type { PmPlanRow } from "@/services/pm-service";
import type { PermissionName } from "@shared/rbac/permissions.generated";

type PlanScope = Pick<PmPlanRow, "access_node_id" | "asset_scope_type">;

/** Plans that span sites: plant-wide rights only. */
const PLANT_WIDE_SCOPE_TYPES = new Set(["family", "criticality_group"]);

/**
 * The org node a plan's rights are read at: its node, or `0` when it has none (the rights at no
 * node are the plant-wide ones only). `null` while no plan is loaded.
 */
export function pmPlanAccessNode(
  plan: Pick<PmPlanRow, "access_node_id"> | null | undefined,
): number | null {
  if (plan == null) return null;
  const node = plan.access_node_id;
  return node != null && node > 0 ? node : 0;
}

/** The user's rights at the plan's site. */
export function usePmPlanAccess(
  plan: Pick<PmPlanRow, "access_node_id"> | null | undefined,
): AccessAt {
  return useAccessAt(pmPlanAccessNode(plan));
}

/** How one plan action is offered. */
export interface PmPlanActionState {
  /** False: the right is held nowhere — the action is not offered at all. */
  show: boolean;
  /** True: held, but not at this plan's site (or still loading). */
  disabled: boolean;
  /** Why it is disabled, for the button's tooltip and the note under the actions. */
  reason: string | null;
}

/** The pm-namespace key of the reason a plan's action is refused to a site-only holder. */
export function pmPlanSiteReasonKey(plan: PlanScope): string {
  if (PLANT_WIDE_SCOPE_TYPES.has(plan.asset_scope_type)) return "hints.siteScopePlantWidePlan";
  if (plan.access_node_id == null || plan.access_node_id <= 0) return "hints.siteScopeNoSite";
  return "hints.siteScopeOtherSite";
}

/** `permission` on the plan: hidden when held nowhere, disabled with a reason when not held here. */
export function usePmPlanActions(
  plan: PlanScope | null | undefined,
): (permission: PermissionName) => PmPlanActionState {
  const { t } = useTranslation("pm");
  const { can: heldAnywhere } = usePermissions();
  const { can: canHere, loading } = usePmPlanAccess(plan);
  return useCallback(
    (permission: PermissionName) => {
      if (!heldAnywhere(permission)) return { show: false, disabled: true, reason: null };
      if (canHere(permission)) return { show: true, disabled: false, reason: null };
      if (loading || plan == null) return { show: true, disabled: true, reason: null };
      return { show: true, disabled: true, reason: t(pmPlanSiteReasonKey(plan)) };
    },
    [heldAnywhere, canHere, loading, plan, t],
  );
}

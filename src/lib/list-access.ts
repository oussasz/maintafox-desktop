/**
 * Lists other modules pick from, and the right the server asks to read each one.
 *
 * A picker fed by a list the user may not read must not ask for it (the server refuses, which used
 * to end in a permission error or the lock screen) and must not look empty "because there is
 * nothing": it says which right is missing (`ListAccessNote`) and filters on it are hidden.
 * The rights mirror the backend commands (`list_personnel`, `search_assets_page`, …).
 */
import { useContext } from "react";

import { PermissionContext } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

export const LIST_READ_RIGHT = {
  /** `list_personnel` */
  people: P.PER_VIEW,
  /** `list_active_teams` */
  teams: P.PER_VIEW,
  /** `list_external_companies` */
  companies: P.PER_VIEW,
  /** `search_assets_page`, `suggest_picker_assets`, `list_assets` */
  assets: P.EQ_VIEW,
  /** `list_inventory_articles` */
  articles: P.INV_VIEW,
  /** `list_inventory_warehouses` */
  warehouses: P.INV_VIEW,
  /** `list_cost_centers` */
  costCentres: P.FIN_VIEW,
  /** `list_org_tree` */
  orgNodes: P.ORG_VIEW,
  /** `list_users` */
  users: P.ADM_USERS,
  /** `list_work_order_types` */
  woTypes: P.REF_VIEW,
} as const satisfies Record<string, PermissionName>;

export type ReadableList = keyof typeof LIST_READ_RIGHT;

/**
 * Whether the signed-in user may read `list`. Outside a permission provider (unit tests) and while
 * permissions load, lists count as readable so nothing flashes a restriction note.
 */
export function useCanReadList(list: ReadableList): boolean {
  const permissions = useContext(PermissionContext);
  if (!permissions || permissions.isLoading) return true;
  return permissions.can(LIST_READ_RIGHT[list]);
}

/** Same as `useCanReadList` for several lists at once. */
export function useReadableLists<K extends ReadableList>(lists: readonly K[]): Record<K, boolean> {
  const permissions = useContext(PermissionContext);
  const out = {} as Record<K, boolean>;
  for (const list of lists) {
    out[list] =
      !permissions || permissions.isLoading ? true : permissions.can(LIST_READ_RIGHT[list]);
  }
  return out;
}

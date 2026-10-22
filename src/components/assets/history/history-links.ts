import type { AssetHistoryEvent } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

export interface HistoryLinkTarget {
  /** Screen the event opens; `null` when the event has no screen. */
  path: string | null;
  /** View right the target screen needs; `null` when the screen has no extra gate. */
  permission: PermissionName | null;
}

// Keep in step with the `PermissionRoute` gates of the modules in `src/router.tsx`.
const ROUTE_RIGHTS: [prefix: string, permission: PermissionName][] = [
  ["/work-orders", P.OT_VIEW],
  ["/requests", P.DI_VIEW],
  ["/pm", P.PM_VIEW],
  ["/inspections", P.INS_VIEW],
  ["/reliability", P.RAM_VIEW],
  ["/equipment", P.EQ_VIEW],
  ["/documentation", P.DOC_VIEW],
];

/**
 * Where an equipment-history event leads and which right that screen needs, so the link is
 * offered only to users who may open it (the history itself needs only the equipment view).
 */
export function historyLinkTarget(ev: AssetHistoryEvent): HistoryLinkTarget {
  let path: string | null = ev.ref?.route_hint ?? null;
  // The backend hint for a linked document predates the documentation module's route.
  if (path === "/documents") path = "/documentation";
  if (!path && ev.ref?.entity_id != null) {
    if (ev.ref.entity_type === "wo") path = `/work-orders?openWo=${ev.ref.entity_id}`;
    else if (ev.ref.entity_type === "di") path = `/requests?openDi=${ev.ref.entity_id}`;
    else if (ev.ref.entity_type === "pm") path = `/pm?planId=${ev.ref.entity_id}`;
  }
  if (!path) return { path: null, permission: null };
  const bare = path.split("?")[0] ?? path;
  const rule = ROUTE_RIGHTS.find(([prefix]) => bare === prefix || bare.startsWith(`${prefix}/`));
  return { path, permission: rule ? rule[1] : null };
}

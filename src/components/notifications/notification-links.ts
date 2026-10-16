/**
 * Notification `action_url` → the in-app route that opens the source record.
 *
 * The backend writes several link shapes over time; each page reads its own query parameter:
 *  - Requests page reads `?openDi=<id>`        (DI notifications already use it; SLA used `?di=`)
 *  - Work orders page reads `?openWo=<id>`     (`/work-orders/<id>`, `/wo/<id>` are older forms)
 *  - PM page reads `?planId=<id>` (a plan) and `?occurrence=<id>` (an occurrence, opened in its
 *    plan; PM notifications carry the occurrence id).
 *  - Budget lives at `/budget` (finance alerts wrote `/finance/budget`).
 *  - Inspection rounds `?openRound=<id>`, articles `?articleId=<id>`, permits `?permit=<id>`: the
 *    pages read them as they are.
 *  - `/sync-center` is not a page: it opens the synchronisation center of the top bar
 *    ([`SYNC_CENTER_LINK`], event [`OPEN_SYNC_CENTER_EVENT`]).
 * This is the one place that knows those forms. Anything that is not an in-app route (external
 * URLs, `javascript:` …) is refused.
 */

export type NotificationLink = { path: string; exact: boolean };

/** Link of the synchronisation conflict notifications. */
export const SYNC_CENTER_LINK = "/sync-center";
/** Window event the top bar listens to, to open its synchronisation center. */
export const OPEN_SYNC_CENTER_EVENT = "mf:open-sync-center";

const ID = /^\d+$/;

function withParam(path: string, key: string, value: string): string {
  return `${path}?${new URLSearchParams({ [key]: value }).toString()}`;
}

/**
 * @returns the route to navigate to and whether it opens the exact record (`exact: false` when
 * only the module can be opened), or `null` when the link is missing or not an in-app route.
 */
export function resolveNotificationLink(
  actionUrl: string | null | undefined,
): NotificationLink | null {
  const raw = actionUrl?.trim();
  if (!raw || !raw.startsWith("/") || raw.startsWith("//")) return null;

  let url: URL;
  try {
    url = new URL(raw, "http://app.local");
  } catch {
    return null;
  }
  const path = url.pathname.replace(/\/+$/, "") || "/";
  const params = url.searchParams;
  const segments = path.split("/").filter(Boolean);

  // Intervention requests
  if (segments[0] === "requests") {
    const id = segments[1] ?? params.get("openDi") ?? params.get("di");
    if (id && ID.test(id)) return { path: withParam("/requests", "openDi", id), exact: true };
    return { path: "/requests", exact: false };
  }

  // Work orders
  if (segments[0] === "work-orders" || segments[0] === "wo") {
    const id = segments[1] ?? params.get("openWo") ?? params.get("wo");
    if (id && ID.test(id)) return { path: withParam("/work-orders", "openWo", id), exact: true };
    return { path: "/work-orders", exact: false };
  }

  // Preventive maintenance
  if (segments[0] === "pm") {
    const occurrence = params.get("occurrence");
    if (occurrence && ID.test(occurrence)) {
      return { path: withParam("/pm", "occurrence", occurrence), exact: true };
    }
    const planId = params.get("planId");
    if (planId && ID.test(planId)) return { path: withParam("/pm", "planId", planId), exact: true };
    return { path: "/pm", exact: false };
  }

  if (path === SYNC_CENTER_LINK) return { path: SYNC_CENTER_LINK, exact: true };

  // Budget
  if (path === "/finance/budget" || path === "/budget") return { path: "/budget", exact: true };

  return { path: `${path}${url.search}`, exact: true };
}

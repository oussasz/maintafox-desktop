/**
 * Where each home figure leads. Every address is a real screen, and every query parameter is one
 * the target page reads on arrival (checked by `src/__tests__/dashboard-drill-links.test.ts`):
 * - `/requests?queue=` — RequestsPage `QUEUE_PARAM` (work queue)
 * - `/work-orders?openWo=` — WorkOrdersPage (opens the OT)
 * - `/inspections?openRound=` — InspectionsPage (opens the round)
 * - `/pm?occurrence=` — PmPage (opens the plan of the occurrence)
 * - `/inventory?articleId=` — InventoryPage (selects the article)
 * - `/permits?permit=` — PermitsPage `PERMIT_QUERY_SELECT`
 * - `/equipment?assetId=` — AssetRegistryPage (selects the asset)
 */
import { PERMIT_QUERY_SELECT } from "@/components/permits/permit-rules";
import type { DiQueue } from "@shared/ipc-types";

export const homeLinks = {
  requests: "/requests",
  requestQueue: (queue: DiQueue) => `/requests?queue=${queue}`,
  workOrders: "/work-orders",
  workOrder: (id: number) => `/work-orders?openWo=${id}`,
  inspections: "/inspections",
  round: (id: number) => `/inspections?openRound=${id}`,
  pm: "/pm",
  pmOccurrence: (id: number) => `/pm?occurrence=${id}`,
  notifications: "/notifications",
  planning: "/planning",
  inventory: "/inventory",
  article: (id: number) => `/inventory?articleId=${id}`,
  permits: "/permits",
  permit: (id: number) => `/permits?${PERMIT_QUERY_SELECT}=${id}`,
  equipment: "/equipment",
  asset: (id: number) => `/equipment?assetId=${id}`,
} as const;

/** Every address a home figure can open (with a sample id), for the route check. */
export function allHomeLinks(): string[] {
  return [
    homeLinks.requests,
    homeLinks.requestQueue("mine"),
    homeLinks.requestQueue("sla_at_risk"),
    homeLinks.workOrders,
    homeLinks.workOrder(1),
    homeLinks.inspections,
    homeLinks.round(1),
    homeLinks.pm,
    homeLinks.pmOccurrence(1),
    homeLinks.notifications,
    homeLinks.planning,
    homeLinks.inventory,
    homeLinks.article(1),
    homeLinks.permits,
    homeLinks.permit(1),
    homeLinks.equipment,
    homeLinks.asset(1),
  ];
}

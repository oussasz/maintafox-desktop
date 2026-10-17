/**
 * Pre-filtered list links: one place that builds and reads the URL filters the lists accept.
 *
 * A dashboard figure opens the list behind it with explicit query parameters; the list reads
 * them into its own filters, so they appear as active filter chips the user can clear — never
 * a hidden filter. Only filters a list can really apply are offered here.
 */
import type { DiQueue } from "@shared/ipc-types";

/** `scope=open`: records still being handled (the dashboards' "open" counts). */
export type ListScope = "open";

export interface WoListLinkFilter {
  scope?: ListScope;
  status?: string;
  urgency?: string;
  equipmentId?: number;
}

export interface DiListLinkFilter {
  queue?: DiQueue;
  scope?: ListScope;
  status?: string;
  urgency?: string;
  equipmentId?: number;
}

export type PmDueLinkWindow = "overdue" | "today" | "next7";

export const LIST_PARAM = {
  scope: "scope",
  status: "status",
  urgency: "urgency",
  equipment: "equipment",
  queue: "queue",
  due: "due",
  stock: "stock",
} as const;

function query(entries: Array<[string, string | number | undefined]>): string {
  const p = new URLSearchParams();
  for (const [k, v] of entries) {
    if (v !== undefined && v !== "") p.set(k, String(v));
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

export const listLinks = {
  workOrders: (f: WoListLinkFilter = {}) =>
    `/work-orders${query([
      [LIST_PARAM.scope, f.scope],
      [LIST_PARAM.status, f.status],
      [LIST_PARAM.urgency, f.urgency],
      [LIST_PARAM.equipment, f.equipmentId],
    ])}`,
  requests: (f: DiListLinkFilter = {}) =>
    `/requests${query([
      [LIST_PARAM.queue, f.queue],
      [LIST_PARAM.scope, f.scope],
      [LIST_PARAM.status, f.status],
      [LIST_PARAM.urgency, f.urgency],
      [LIST_PARAM.equipment, f.equipmentId],
    ])}`,
  pmDue: (window: PmDueLinkWindow) => `/pm${query([[LIST_PARAM.due, window]])}`,
  equipment: (f: { status?: string } = {}) => `/equipment${query([[LIST_PARAM.status, f.status]])}`,
  inventory: (f: { belowReorder?: boolean } = {}) =>
    `/inventory${query([[LIST_PARAM.stock, f.belowReorder ? "below_reorder" : undefined]])}`,
} as const;

/** Positive integer id from a query value, else null. */
export function idParam(raw: string | null): number | null {
  if (raw == null) return null;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? n : null;
}

/** Non-empty text value, else null. */
export function textParam(raw: string | null): string | null {
  const v = raw?.trim();
  return v ? v : null;
}

export function scopeParam(raw: string | null): ListScope | null {
  return raw === "open" ? "open" : null;
}

export function dueWindowParam(raw: string | null): PmDueLinkWindow | null {
  return raw === "overdue" || raw === "today" || raw === "next7" ? raw : null;
}

/** Copy of `params` without `keys` (clearing a chip removes its parameter from the link). */
export function withoutParams(params: URLSearchParams, ...keys: string[]): URLSearchParams {
  const next = new URLSearchParams(params);
  for (const k of keys) next.delete(k);
  return next;
}

/**
 * Client-side mirror of the permit safety rules enforced in
 * `src-tauri/src/permit/queries.rs`. Used only to explain to the user *why* an
 * action is unavailable; the backend remains the authority and re-checks all of it.
 */

import type { PermitIsolationRecord, WorkPermitRecord } from "@/services/permit-service";
import type { WorkPermitStatus } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** `/permits` query params used by the WO detail launch point. */
export const PERMIT_QUERY_SELECT = "permit";
export const PERMIT_QUERY_CREATE_FOR_WO = "createForWo";

export type IsolationState = "pending" | "applied" | "verified" | "removed";

export function isolationState(iso: PermitIsolationRecord): IsolationState {
  if (iso.removal_verified_at) return "removed";
  if (iso.verified_at) return "verified";
  if (iso.applied_at) return "applied";
  return "pending";
}

/** Statuses in which isolations may be defined, applied and verified. */
export const ISOLATION_SETUP_STATUSES: ReadonlySet<string> = new Set([
  "draft",
  "pending_review",
  "approved",
  "issued",
  "suspended",
  "revalidation_required",
]);

/** Statuses in which lock removal may be verified. */
export const ISOLATION_REMOVAL_STATUSES: ReadonlySet<string> = new Set([
  "active",
  "cancelled",
  "expired",
]);

/** Header fields (expiry) are editable only before activation. */
export const PERMIT_EDITABLE_STATUSES: ReadonlySet<string> = new Set([
  "draft",
  "pending_review",
  "approved",
  "issued",
]);

export function isPermitExpired(permit: WorkPermitRecord, now: Date = new Date()): boolean {
  return permit.expires_at != null && new Date(permit.expires_at).getTime() <= now.getTime();
}

/** i18n key (in the `permits` namespace) explaining why activation is blocked, or null. */
export function activationBlocker(
  permit: WorkPermitRecord,
  isolations: PermitIsolationRecord[],
): string | null {
  if (isPermitExpired(permit)) return "gate.expired";
  if (permit.permit_type_code === "loto" && isolations.length === 0) return "gate.noIsolation";
  if (isolations.some((i) => i.removal_verified_at)) return "gate.removed";
  if (isolations.some((i) => !i.verified_at)) return "gate.unverified";
  return null;
}

/** i18n key explaining why closing is blocked, or null. */
export function closeBlocker(isolations: PermitIsolationRecord[]): string | null {
  return isolations.some((i) => !i.removal_verified_at) ? "gate.pendingRemoval" : null;
}

export interface PermitTransition {
  to: WorkPermitStatus;
  labelKey: string;
  /** Close-type transitions require `ptw.close`; everything else `ptw.issue`. */
  permission: typeof P.PTW_ISSUE | typeof P.PTW_CLOSE;
}

/** Forward transitions offered by `set_work_permit_status` (suspend has its own action). */
export function nextTransitions(status: string): PermitTransition[] {
  switch (status) {
    case "draft":
      return [
        { to: "pending_review", labelKey: "transition.submitReview", permission: P.PTW_ISSUE },
      ];
    case "pending_review":
      return [{ to: "approved", labelKey: "transition.approve", permission: P.PTW_ISSUE }];
    case "approved":
      return [{ to: "issued", labelKey: "transition.issue", permission: P.PTW_ISSUE }];
    case "issued":
      return [{ to: "active", labelKey: "transition.activate", permission: P.PTW_ISSUE }];
    case "suspended":
      return [
        { to: "revalidation_required", labelKey: "transition.revalidate", permission: P.PTW_ISSUE },
      ];
    case "revalidation_required":
      return [{ to: "active", labelKey: "transition.reinstate", permission: P.PTW_ISSUE }];
    case "active":
      return [{ to: "closed", labelKey: "transition.close", permission: P.PTW_CLOSE }];
    case "closed":
      return [{ to: "handed_back", labelKey: "transition.handBack", permission: P.PTW_CLOSE }];
    default:
      return [];
  }
}

const NON_CANCELLABLE: ReadonlySet<string> = new Set(["closed", "cancelled", "expired"]);

export function canCancel(status: string): boolean {
  return !NON_CANCELLABLE.has(status);
}

/** `datetime-local` value (local time) → RFC 3339 UTC, or null when empty/invalid. */
export function localInputToIso(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/** Date → `datetime-local` input value in local time. */
export function toLocalInputValue(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(
    d.getMinutes(),
  )}`;
}

export function formatAssetLabel(permit: WorkPermitRecord): string | null {
  if (permit.asset_code && permit.asset_label)
    return `${permit.asset_code} — ${permit.asset_label}`;
  return permit.asset_code ?? permit.asset_label ?? null;
}

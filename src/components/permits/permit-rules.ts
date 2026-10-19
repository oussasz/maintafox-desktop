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

/**
 * Namespaces holding the texts of the permit refusals (`training:errors.PERMIT_*`). They are
 * loaded on demand, so the permit screens load them up front: a refusal shown before any other
 * screen opened them is then in the user's language, not the server's English sentence.
 */
export const PERMIT_REFUSAL_NAMESPACES = ["training"] as const;

export type IsolationState = "pending" | "applied" | "verified" | "removed" | "cancelled";

/**
 * A planned line cancelled and never applied (`COUNTED_ISOLATION_SQL` on the server): kept as
 * history, no isolation of its permit. A line that would hold both still counts.
 */
export function isCancelledLine(iso: PermitIsolationRecord): boolean {
  return iso.cancelled_at != null && iso.applied_at == null;
}

/** The lines that count as isolations of the permit (cancelled planned lines left out). */
export function countedIsolations(isolations: PermitIsolationRecord[]): PermitIsolationRecord[] {
  return isolations.filter((i) => !isCancelledLine(i));
}

export function isolationState(iso: PermitIsolationRecord): IsolationState {
  if (isCancelledLine(iso)) return "cancelled";
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

/**
 * Statuses in which a planned line may be cancelled (`ISOLATION_CANCEL_STATUSES`): a cancellation
 * while waiting for review sends the permit back to draft.
 */
export const ISOLATION_CANCEL_STATUSES: ReadonlySet<string> = new Set(["draft", "pending_review"]);

/** Whether the line may be cancelled (rights aside): never applied, not cancelled, permit prepared. */
export function canCancelIsolation(permitStatus: string, iso: PermitIsolationRecord): boolean {
  return (
    ISOLATION_CANCEL_STATUSES.has(permitStatus) &&
    iso.cancelled_at == null &&
    iso.applied_at == null &&
    iso.applied_by_id == null &&
    iso.verified_at == null &&
    iso.removal_verified_at == null
  );
}

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
  allIsolations: PermitIsolationRecord[],
): string | null {
  const isolations = countedIsolations(allIsolations);
  if (isPermitExpired(permit)) return "gate.expired";
  // The type's own rule (`requires_energy_isolation`, set for LOTO), never a code in the program.
  if (permit.permit_type_requires_energy_isolation && isolations.length === 0)
    return "gate.noIsolation";
  if (isolations.some((i) => i.removal_verified_at)) return "gate.removed";
  if (isolations.some((i) => !i.verified_at)) return "gate.unverified";
  return null;
}

/** i18n key explaining why closing is blocked, or null. */
export function closeBlocker(isolations: PermitIsolationRecord[]): string | null {
  return countedIsolations(isolations).some((i) => !i.removal_verified_at)
    ? "gate.pendingRemoval"
    : null;
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

/** Isolation choice label in the UI language: a shipped value's English label, else its label. */
export function optionLabel(
  option: { label: string; label_en: string | null } | null | undefined,
  language: string,
): string {
  if (!option) return "";
  return language.toLowerCase().startsWith("en") && option.label_en
    ? option.label_en
    : option.label;
}

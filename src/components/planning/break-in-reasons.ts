/**
 * Break-in reasons: the COMPANY list PLANNING.BREAK_IN_REASONS of the reference data (each plant
 * words and extends its own). A reason tagged `planning_break_in_urgent` (the starter emergency
 * and safety) needs an approver or a recorded dangerous-override reason; the frontend keeps no
 * list of its own.
 */

import { useEffect, useState } from "react";

import { natureValueLabel } from "@/components/lookups/MaintenanceNaturePicker";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { ReferenceValue } from "@shared/ipc-types";

export const BREAK_IN_REASONS_DOMAIN = "PLANNING.BREAK_IN_REASONS";
export const BREAK_IN_URGENT_TAG = "planning_break_in_urgent";

export function isUrgentBreakInReason(value: ReferenceValue | undefined): boolean {
  return value?.semantic_tag === BREAK_IN_URGENT_TAG;
}

/** Label of a reason code in the UI language; the code itself when it is not in the list. */
export function breakInReasonLabel(
  code: string,
  reasons: ReferenceValue[],
  language: string,
): string {
  const value = reasons.find((r) => r.code === code);
  return value ? natureValueLabel(value, language) : code;
}

/** The active break-in reasons; loaded only when `enabled`. */
export function useBreakInReasons(enabled: boolean): ReferenceValue[] {
  const [reasons, setReasons] = useState<ReferenceValue[]>([]);
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void listPublishedReferenceValuesByDomainCode(BREAK_IN_REASONS_DOMAIN)
      .then((rows) => {
        if (!cancelled) setReasons(rows.filter((r) => r.is_active));
      })
      .catch(() => {
        // The global toast reports the failure; the dialog then offers no reason (no submit).
        if (!cancelled) setReasons([]);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);
  return reasons;
}

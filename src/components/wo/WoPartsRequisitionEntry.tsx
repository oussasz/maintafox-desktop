/**
 * "Créer une demande d'achat" on the work order sheet (L6-11): the parts shortage of the work
 * order with the same button as the planning panel (`WoShortageSection`), reachable from any tab
 * while the work order is still open. Nothing is shown without `inv.procure`, on a closed or
 * cancelled order, or when no part is short.
 */

import { useEffect, useState } from "react";

import { WoShortageSection } from "@/components/wo/WoShortageSection";
import { usePlantWideAccess } from "@/components/wo/WoSiteScope";
import { checkWoPartStockAvailability } from "@/services/inventory-service";
import type { WoMaterialReadiness } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Statuses that still use parts: a request raised from them can still serve the work order. */
export const REQUISITION_ENTRY_STATES: ReadonlySet<string> = new Set([
  "planning",
  "ready",
  "in_progress",
  "on_hold",
]);

interface WoPartsRequisitionEntryProps {
  woId: number;
  statusCode: string;
}

export function WoPartsRequisitionEntry({ woId, statusCode }: WoPartsRequisitionEntryProps) {
  // The stock check needs inv.view and the request inv.procure (both plant-wide, as the backend).
  const access = usePlantWideAccess();
  const canProcure = access.can(P.INV_PROCURE) && access.can(P.INV_VIEW);
  const open = REQUISITION_ENTRY_STATES.has(statusCode);
  const [readiness, setReadiness] = useState<WoMaterialReadiness | null>(null);

  useEffect(() => {
    if (!canProcure || !open) {
      setReadiness(null);
      return;
    }
    let cancelled = false;
    void checkWoPartStockAvailability(woId)
      .then((result) => {
        if (!cancelled) setReadiness(result);
      })
      .catch(() => {
        if (!cancelled) setReadiness(null);
      });
    return () => {
      cancelled = true;
    };
  }, [woId, canProcure, open]);

  if (!canProcure || !open) return null;
  return <WoShortageSection woId={woId} readiness={readiness} />;
}

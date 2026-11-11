import { useCallback, useEffect, useState } from "react";

import { usePermissions } from "@/hooks/use-permissions";
import { getLateWorkOrderCount, getOverdueRequestCount } from "@/services/bell-alerts-service";
import { P } from "@shared/rbac/permissions.generated";

/** The statistics are heavier than the notification counts: read them less often. */
export const BELL_ALERTS_POLL_MS = 120_000;

export interface BellAlerts {
  /** Late work orders; null when the user has no work order right (or the read failed). */
  lateWorkOrders: number | null;
  /** Overdue requests; null when the user has no request right (or the read failed). */
  overdueRequests: number | null;
  /** Reads the figures again (the bell does it when it opens). */
  refresh: () => Promise<void>;
}

/**
 * Business alerts of the bell. A user without `ot.view` (or `di.view` with a screening, review or
 * approval right) never asks for (and never gets)
 * the matching figure; the commands themselves count only the user's sites.
 */
export function useBellAlerts(active: boolean): BellAlerts {
  const { can } = usePermissions();
  const canWo = can(P.OT_VIEW);
  // The late-requests queue the row opens is shown to screeners, reviewers and approvers only
  // (`DiQueueBar`): others would land on a list that does not match the figure.
  const canDi = can(P.DI_VIEW) && (can(P.DI_SCREEN) || can(P.DI_REVIEW) || can(P.DI_APPROVE));
  const [lateWorkOrders, setLate] = useState<number | null>(null);
  const [overdueRequests, setOverdue] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    if (!active) return;
    const [wo, di] = await Promise.all([
      canWo ? getLateWorkOrderCount().catch(() => null) : Promise.resolve(null),
      canDi ? getOverdueRequestCount().catch(() => null) : Promise.resolve(null),
    ]);
    setLate(wo);
    setOverdue(di);
  }, [active, canWo, canDi]);

  useEffect(() => {
    if (!active) {
      setLate(null);
      setOverdue(null);
      return;
    }
    void refresh();
    const id = window.setInterval(() => void refresh(), BELL_ALERTS_POLL_MS);
    return () => window.clearInterval(id);
  }, [active, refresh]);

  return { lateWorkOrders, overdueRequests, refresh };
}

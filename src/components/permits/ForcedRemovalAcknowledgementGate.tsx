/**
 * Blocking screen at sign-in: a lock of the signed-in worker was taken off by a forced removal
 * while they were absent (OSHA 1910.147(e)(3)(ii) C). The worker must acknowledge each one before
 * doing anything else in the app; the acknowledgement records them as told, "in-app
 * acknowledgement", audited (`acknowledge_forced_lock_removal`). Until then the backend refuses
 * them as the applier of any new lock. Nothing else of the app can be reached while one is shown.
 */

import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { appErrorReason } from "@/lib/app-error";
import {
  type ForcedRemovalItem,
  acknowledgeForcedLockRemoval,
  listMyForcedLockRemovals,
} from "@/services/permit-service";
import { formatDateTime } from "@/utils/format-date";

import { PERMIT_REFUSAL_NAMESPACES } from "./permit-rules";

interface ForcedRemovalAcknowledgementGateProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
}

export function ForcedRemovalAcknowledgementGate({
  sessionUserId,
}: ForcedRemovalAcknowledgementGateProps) {
  const { t, i18n } = useTranslation(["permits", ...PERMIT_REFUSAL_NAMESPACES]);
  const [items, setItems] = useState<ForcedRemovalItem[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (isCancelled: () => boolean) => {
    try {
      const next = await listMyForcedLockRemovals();
      if (!isCancelled()) setItems(next);
    } catch {
      // Not readable now: the backend still refuses new locks until it is acknowledged.
      if (!isCancelled()) setItems([]);
    }
  }, []);

  useEffect(() => {
    setError(null);
    if (sessionUserId === null) {
      setItems([]);
      return;
    }
    let cancelled = false;
    void load(() => cancelled);
    return () => {
      cancelled = true;
    };
  }, [sessionUserId, load]);

  if (items.length === 0) return null;

  const fmt = (v: string | null) =>
    v ? formatDateTime(v, i18n.language) : t("permits:detail.none");
  const acknowledge = async (isolationId: number) => {
    setBusyId(isolationId);
    setError(null);
    try {
      await acknowledgeForcedLockRemoval(isolationId);
      setItems((current) => current.filter((i) => i.isolation.id !== isolationId));
    } catch (err) {
      setError(appErrorReason(err));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="forced-removal-gate-title"
      data-testid="forced-removal-gate"
      className="fixed inset-0 z-[90] flex items-center justify-center bg-surface-0/95 p-4"
    >
      <div className="max-h-full w-full max-w-2xl space-y-4 overflow-auto rounded-lg border border-tone-danger-border bg-surface-1 p-5 shadow-lg">
        <div className="flex items-start gap-3">
          <ShieldAlert className="mt-0.5 h-6 w-6 shrink-0 text-tone-danger-fg" aria-hidden />
          <div className="space-y-1">
            <h2 id="forced-removal-gate-title" className="text-lg font-semibold text-text-primary">
              {t("permits:gateAck.title")}
            </h2>
            <p className="text-sm text-text-secondary">{t("permits:gateAck.intro")}</p>
          </div>
        </div>
        <ul className="space-y-3">
          {items.map((item) => {
            const iso = item.isolation;
            return (
              <li
                key={iso.id}
                data-testid="forced-removal-gate-item"
                className="space-y-2 rounded-md border border-surface-border p-3 text-sm"
              >
                <p className="font-medium text-text-primary">
                  {t("permits:gateAck.lock", {
                    point: iso.isolation_point,
                    permit: item.permit_code,
                    equipment: item.equipment_label ?? t("permits:detail.none"),
                  })}
                </p>
                <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
                  <dt className="text-text-muted">{t("permits:forced.removedAt")}</dt>
                  <dd>{fmt(iso.removed_at)}</dd>
                  <dt className="text-text-muted">{t("permits:forced.removedBy")}</dt>
                  <dd>{iso.removed_by_name ?? t("permits:isolation.personUnknown")}</dd>
                  <dt className="text-text-muted">{t("permits:forced.authorisedBy")}</dt>
                  <dd>{iso.forced_authorized_by_name ?? t("permits:isolation.personUnknown")}</dd>
                  <dt className="text-text-muted">{t("permits:forced.reason")}</dt>
                  <dd>{iso.forced_reason ?? t("permits:detail.none")}</dd>
                </dl>
                <Button
                  type="button"
                  disabled={busyId === iso.id}
                  onClick={() => void acknowledge(iso.id)}
                >
                  {t("permits:gateAck.acknowledge")}
                </Button>
              </li>
            );
          })}
        </ul>
        {error ? (
          <p role="alert" className="text-sm text-tone-danger-fg">
            {error}
          </p>
        ) : null}
      </div>
    </div>
  );
}

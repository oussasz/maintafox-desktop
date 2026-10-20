/**
 * Blocking screen at sign-in: a lock of the signed-in worker was taken off by a forced removal
 * while they were absent (OSHA 1910.147(e)(3)(ii) C). The worker must acknowledge each one before
 * doing anything else in the app; the acknowledgement records them as told, "in-app
 * acknowledgement", audited (`acknowledge_forced_lock_removal`). Until then the backend refuses
 * them as the applier of any new lock.
 *
 * Nothing else of the app can be reached while it is shown: the shell makes the rest of the app
 * inert (`onActiveChange`), focus stays inside the screen (Tab and Shift+Tab cycle through it),
 * and it sits above every popover and dialog layer. When the list cannot be read, the screen says
 * so and offers to try again rather than letting the worker past.
 */

import { ShieldAlert } from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
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

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface ForcedRemovalAcknowledgementGateProps {
  /** Re-read when the signed-in account changes (its user id, or null when signed out). */
  sessionUserId: number | null;
  /** Told whether the screen is shown (the shell makes the rest of the app inert meanwhile). */
  onActiveChange?: (active: boolean) => void;
}

export function ForcedRemovalAcknowledgementGate({
  sessionUserId,
  onActiveChange,
}: ForcedRemovalAcknowledgementGateProps) {
  const { t, i18n } = useTranslation(["permits", ...PERMIT_REFUSAL_NAMESPACES]);
  const [items, setItems] = useState<ForcedRemovalItem[]>([]);
  const [loadFailed, setLoadFailed] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const load = useCallback(async (isCancelled: () => boolean) => {
    setLoading(true);
    try {
      const next = await listMyForcedLockRemovals();
      if (!isCancelled()) {
        setItems(next);
        setLoadFailed(false);
      }
    } catch {
      // Not readable now: say so and let the worker try again (the backend still refuses new
      // locks until the forced removal is acknowledged).
      if (!isCancelled()) {
        setItems([]);
        setLoadFailed(true);
      }
    } finally {
      if (!isCancelled()) setLoading(false);
    }
  }, []);

  useEffect(() => {
    setError(null);
    if (sessionUserId === null) {
      setItems([]);
      setLoadFailed(false);
      return;
    }
    let cancelled = false;
    void load(() => cancelled);
    return () => {
      cancelled = true;
    };
  }, [sessionUserId, load]);

  const active = sessionUserId !== null && (items.length > 0 || loadFailed);

  useEffect(() => {
    onActiveChange?.(active);
  }, [active, onActiveChange]);
  useEffect(() => () => onActiveChange?.(false), [onActiveChange]);

  // Focus moves into the screen when it shows (and back into it if it was lost).
  useEffect(() => {
    if (!active) return;
    const panel = panelRef.current;
    if (panel && !panel.contains(document.activeElement)) {
      const first = panel.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? panel).focus();
    }
  }, [active, items.length, loadFailed]);

  if (!active) return null;

  /** Tab and Shift+Tab cycle inside the screen; Escape does nothing. */
  const trapFocus = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      return;
    }
    if (e.key !== "Tab") return;
    const panel = panelRef.current;
    const focusable = Array.from(panel?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) {
      e.preventDefault();
      return;
    }
    const current = document.activeElement;
    if (e.shiftKey && (current === first || !panel?.contains(current))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (current === last || !panel?.contains(current))) {
      e.preventDefault();
      first.focus();
    }
  };

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
      // Above every popover (z-200) and dialog layer of the app.
      className="fixed inset-0 z-[300] flex items-center justify-center bg-surface-0/95 p-4"
      onKeyDown={trapFocus}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className="max-h-full w-full max-w-2xl space-y-4 overflow-auto rounded-lg border border-tone-danger-border bg-surface-1 p-5 shadow-lg outline-none"
      >
        <div className="flex items-start gap-3">
          <ShieldAlert className="mt-0.5 h-6 w-6 shrink-0 text-tone-danger-fg" aria-hidden />
          <div className="space-y-1">
            <h2 id="forced-removal-gate-title" className="text-lg font-semibold text-text-primary">
              {t("permits:gateAck.title")}
            </h2>
            <p className="text-sm text-text-secondary">{t("permits:gateAck.intro")}</p>
          </div>
        </div>
        {loadFailed ? (
          <div className="space-y-2" data-testid="forced-removal-gate-error">
            <p role="alert" className="text-sm text-tone-danger-fg">
              {t("permits:gateAck.loadError")}
            </p>
            <Button type="button" disabled={loading} onClick={() => void load(() => false)}>
              {t("permits:gateAck.retry")}
            </Button>
          </div>
        ) : null}
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

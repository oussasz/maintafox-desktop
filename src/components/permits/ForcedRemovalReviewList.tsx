/**
 * Review list of the permit close-right holders: locks taken off by a forced removal whose worker
 * has not been told yet, at the sites where the user holds `ptw.close` (and those the user
 * authorised), closed permits included (`list_open_forced_lock_removals`). Each item stays until
 * the worker acknowledges it at sign-in or a holder records that they were told ("Travailleur
 * informé").
 */

import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { cn } from "@/lib/utils";
import {
  type ForcedRemovalItem,
  type PermitIsolationRecord,
  listOpenForcedLockRemovals,
} from "@/services/permit-service";
import { formatDateTime } from "@/utils/format-date";

import { WorkerInformedDialog } from "./WorkerInformedDialog";

interface ForcedRemovalReviewListProps {
  /** Opens the permit of an item. */
  onOpenPermit: (permitId: number) => void;
  /** Changes when the page reloads (an item may have been settled in the permit panel). */
  reloadKey?: unknown;
  className?: string;
}

export function ForcedRemovalReviewList({
  onOpenPermit,
  reloadKey,
  className,
}: ForcedRemovalReviewListProps) {
  const { t, i18n } = useTranslation("permits");
  const [items, setItems] = useState<ForcedRemovalItem[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [informing, setInforming] = useState<PermitIsolationRecord | null>(null);

  const load = useCallback(async () => {
    try {
      setItems(await listOpenForcedLockRemovals());
      setError(null);
    } catch (e) {
      setError(e);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, reloadKey]);

  if (error != null) {
    return (
      <ErrorState
        error={error}
        title={t("review.loadError")}
        onRetry={() => void load()}
        className={className ?? ""}
      />
    );
  }
  if (items.length === 0) return null;

  const fmt = (v: string | null) => (v ? formatDateTime(v, i18n.language) : t("detail.none"));
  return (
    <section
      aria-labelledby="forced-review-title"
      data-testid="forced-removal-review"
      className={cn(
        "space-y-2 rounded-md border border-tone-warning-border bg-tone-warning-bg p-3",
        className,
      )}
    >
      <h3
        id="forced-review-title"
        className="flex items-center gap-2 text-sm font-semibold text-tone-warning-fg"
      >
        <ShieldAlert className="h-4 w-4" aria-hidden />
        {t("review.title", { count: items.length })}
      </h3>
      <p className="text-xs text-text-secondary">{t("review.hint")}</p>
      <ul className="space-y-1">
        {items.map((item) => {
          const iso = item.isolation;
          return (
            <li
              key={iso.id}
              data-testid="forced-removal-review-item"
              className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm"
            >
              <Button
                type="button"
                variant="link"
                className="h-auto p-0"
                onClick={() => onOpenPermit(item.permit_id)}
              >
                {item.permit_code}
              </Button>
              <span>
                {t("review.item", {
                  point: iso.isolation_point,
                  worker: iso.applied_by_name ?? t("isolation.personUnknown"),
                  date: fmt(iso.removed_at),
                  authoriser: iso.forced_authorized_by_name ?? t("isolation.personUnknown"),
                })}
              </span>
              <span className="text-xs text-text-muted">
                {t(`status.${item.permit_status}`, { defaultValue: t("status.unknown") })}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="ml-auto"
                onClick={() => setInforming(iso)}
              >
                {t("informed.action")}
              </Button>
            </li>
          );
        })}
      </ul>
      <WorkerInformedDialog
        isolation={informing}
        onOpenChange={(open) => {
          if (!open) setInforming(null);
        }}
        onRecorded={() => void load()}
      />
    </section>
  );
}

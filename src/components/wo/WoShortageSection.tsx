/**
 * Parts shortage of a work order, with "Créer une demande d'achat" per short article.
 *
 * One block, two places: the planning panel (preparation) and the work order sheet itself
 * (`WoPartsRequisitionEntry`). Same command (`create_procurement_requisition_from_wo_part`), same
 * pre-fill from the work order's part lines. The backend requires `inv.procure` plant-wide, so the
 * block is offered only with that right and is hidden without it.
 */

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { usePlantWideAccess } from "@/components/wo/WoSiteScope";
import { describeAppError } from "@/lib/app-error";
import { createProcurementRequisitionFromWoPart } from "@/services/inventory-service";
import type { WoMaterialReadiness } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface WoShortageSectionProps {
  woId: number;
  readiness: WoMaterialReadiness | null;
  /** Called with the message when a creation fails. */
  onError?: (message: string) => void;
}

export function WoShortageSection({ woId, readiness, onError }: WoShortageSectionProps) {
  const { t } = useTranslation("ot");
  const access = usePlantWideAccess();
  const [creating, setCreating] = useState<number | null>(null);
  const [createdIds, setCreatedIds] = useState<number[]>([]);
  const [localError, setLocalError] = useState<string | null>(null);

  const create = useCallback(
    (articleId: number, qty: number, locationId: number | null | undefined) => {
      setCreating(articleId);
      setLocalError(null);
      void createProcurementRequisitionFromWoPart(woId, articleId, qty, locationId)
        .then(() => setCreatedIds((prev) => [...prev, articleId]))
        .catch((err) => {
          const message = describeAppError(err).title;
          setLocalError(message);
          onError?.(message);
        })
        .finally(() => setCreating(null));
    },
    [woId, onError],
  );

  if (!access.can(P.INV_PROCURE)) return null;
  if (!readiness || readiness.is_fully_available || readiness.shortages.length === 0) return null;

  return (
    <section className="space-y-2 rounded-md border border-tone-warning-border bg-tone-warning-bg p-3">
      <h3 className="text-sm font-semibold text-tone-warning-fg">{t("planning.shortage.title")}</h3>
      <p className="text-xs text-tone-warning-fg">{t("planning.shortage.description")}</p>
      <div className="space-y-1.5">
        {readiness.shortages.map((shortage) => {
          const wasCreated = createdIds.includes(shortage.article_id);
          return (
            <div
              key={shortage.article_id}
              className="flex flex-wrap items-center gap-2 rounded border border-tone-warning-border bg-surface-0 p-2 text-xs"
            >
              <span className="font-medium">{shortage.article_code}</span>
              <span className="text-muted-foreground">— {shortage.article_name}</span>
              <span className="ml-auto tabular-nums text-tone-warning-fg">
                {t("planning.shortage.columns.requested")}: {shortage.requested_qty} ·{" "}
                {t("planning.shortage.columns.available")}: {shortage.available_qty} ·{" "}
                <span className="font-semibold text-tone-danger-fg">
                  {t("planning.shortage.columns.shortage")}: {shortage.shortage_qty}
                </span>
              </span>
              {wasCreated ? (
                <span className="text-xs font-medium text-tone-success-fg">
                  ✓ {t("planning.shortage.requisitionCreated")}
                </span>
              ) : (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-6 px-2 text-xs"
                  disabled={creating === shortage.article_id}
                  onClick={() =>
                    create(
                      shortage.article_id,
                      shortage.shortage_qty,
                      shortage.preferred_location_id,
                    )
                  }
                >
                  {creating === shortage.article_id
                    ? "…"
                    : t("planning.shortage.createRequisition")}
                </Button>
              )}
            </div>
          );
        })}
      </div>
      {localError ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {localError}
        </p>
      ) : null}
    </section>
  );
}

/**
 * Parts shortage of a work order, with "Créer une demande d'achat" per short article.
 *
 * One block, two places: the planning panel (preparation) and the work order sheet itself
 * (`WoPartsRequisitionEntry`). Same command (`create_procurement_requisition_from_wo_part`), same
 * pre-fill from the work order's part lines. The backend requires `inv.procure` plant-wide, so the
 * button is offered only with that right; the shortage rows stay visible to planners.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { usePlantWideAccess } from "@/components/wo/WoSiteScope";
import { describeAppError } from "@/lib/app-error";
import {
  createProcurementRequisitionFromWoPart,
  listWorkOrderRequisitionNumbers,
} from "@/services/inventory-service";
import type { WoMaterialReadiness } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface WoShortageSectionProps {
  woId: number;
  readiness: WoMaterialReadiness | null;
}

export function WoShortageSection({ woId, readiness }: WoShortageSectionProps) {
  const { t } = useTranslation("ot");
  // The short parts are information for every planner; only the request button needs inv.procure.
  const access = usePlantWideAccess();
  const canProcure = access.can(P.INV_PROCURE);
  const canView = access.can(P.INV_VIEW);
  const [creating, setCreating] = useState<number | null>(null);
  const [created, setCreated] = useState<Record<number, string>>({});
  const [localError, setLocalError] = useState<string | null>(null);
  // Requests already raised for this work order (durable: survive a tab switch or a reopen).
  const [existing, setExisting] = useState<Record<number, string>>({});
  const hasShortage = readiness != null && readiness.shortages.length > 0;

  useEffect(() => {
    if (!canView || !hasShortage) return;
    let cancelled = false;
    void listWorkOrderRequisitionNumbers(woId)
      .then((numbers) => {
        if (!cancelled) setExisting(numbers);
      })
      .catch(() => {
        if (!cancelled) setExisting({});
      });
    return () => {
      cancelled = true;
    };
  }, [woId, canView, hasShortage]);

  const create = useCallback(
    (articleId: number, qty: number, locationId: number | null | undefined) => {
      setCreating(articleId);
      setLocalError(null);
      void createProcurementRequisitionFromWoPart(woId, articleId, qty, locationId)
        .then((req) => setCreated((prev) => ({ ...prev, [articleId]: req.req_number })))
        .catch((err) => {
          setLocalError(describeAppError(err).title);
        })
        .finally(() => setCreating(null));
    },
    [woId],
  );

  if (!readiness || readiness.is_fully_available || readiness.shortages.length === 0) return null;

  return (
    <section className="space-y-2 rounded-md border border-tone-warning-border bg-tone-warning-bg p-3">
      <h3 className="text-sm font-semibold text-tone-warning-fg">{t("planning.shortage.title")}</h3>
      <p className="text-xs text-tone-warning-fg">{t("planning.shortage.description")}</p>
      <div className="space-y-1.5">
        {readiness.shortages.map((shortage) => {
          const createdNumber = created[shortage.article_id];
          const wasCreated = createdNumber !== undefined;
          const inProgressNumber = existing[shortage.article_id];
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
                  ✓{" "}
                  {createdNumber
                    ? t("planning.shortage.requisitionCreatedNumber", { number: createdNumber })
                    : t("planning.shortage.requisitionCreated")}
                </span>
              ) : inProgressNumber !== undefined ? (
                <span className="text-xs font-medium text-tone-success-fg">
                  {t("planning.shortage.requisitionInProgress", { number: inProgressNumber })}
                </span>
              ) : canProcure ? (
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
              ) : null}
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

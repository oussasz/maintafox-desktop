/**
 * Sync centre: stock locations left below zero by received movements, counted twice on two PCs,
 * reservations issued or PO lines received beyond their quantity by several PCs, and
 * over-reservations.
 *
 * They are not sync conflicts (the movements are applied: nothing to accept or reject); the
 * notice points to the list where they are reconciled (Inventory › Controls). The counts come
 * from the stock balances and the count sessions (`get_negative_stock_summary`, inv.view).
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { mfAlert } from "@/design-system/tokens";
import { listLinks } from "@/lib/list-links";
import {
  type NegativeStockSummary,
  getNegativeStockSummary,
} from "@/services/negative-stock-service";

export interface NegativeStockSyncNoticeProps {
  /** Called when the user follows the link (the sync centre closes). */
  onNavigate?: () => void;
}

export function NegativeStockSyncNotice({ onNavigate }: NegativeStockSyncNoticeProps) {
  const { t } = useTranslation("shell");
  // The over-issue / over-receipt line is worded with the stock to reconcile (inventory texts).
  const { t: ti } = useTranslation("inventory");
  const [summary, setSummary] = useState<NegativeStockSummary | null>(null);

  useEffect(() => {
    let cancelled = false;
    getNegativeStockSummary()
      .then((s) => {
        if (!cancelled) setSummary(s);
      })
      .catch(() => {
        // The global IPC toast reports the failure; the notice stays hidden.
        if (!cancelled) setSummary(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (
    !summary ||
    (summary.negative_count === 0 &&
      summary.double_count_count === 0 &&
      summary.double_posting_count === 0 &&
      summary.over_quantity_count === 0 &&
      summary.over_reserved_count === 0)
  ) {
    return null;
  }
  return (
    <div className={summary.negative_count > 0 ? mfAlert.danger : mfAlert.warning} role="status">
      {summary.negative_count > 0 ? (
        <p>{t("sync.negativeStock", { count: summary.negative_count })}</p>
      ) : null}
      {summary.double_count_count > 0 ? (
        <p>{t("sync.doubleCount", { count: summary.double_count_count })}</p>
      ) : null}
      {summary.double_posting_count > 0 ? (
        <p>{t("sync.doublePosting", { count: summary.double_posting_count })}</p>
      ) : null}
      {summary.over_quantity_count > 0 ? (
        <p>{ti("negativeStock.overQuantity.syncNotice", { count: summary.over_quantity_count })}</p>
      ) : null}
      {summary.over_reserved_count > 0 ? (
        <p>{t("sync.overReserved", { count: summary.over_reserved_count })}</p>
      ) : null}
      <Link
        to={listLinks.inventory({ toReconcile: true })}
        className="mt-1 inline-block text-xs font-medium underline"
        onClick={onNavigate}
      >
        {t("sync.openNegativeStock")}
      </Link>
    </div>
  );
}

/**
 * Status of a hors-stock part line on a work order (L3-04): bought for this work order, never
 * reserved or issued from the store; it counts in the parts actual as it is received.
 */

import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";

/** Code of the hors-stock value of the system list `inventory.stocking_type`. */
export const NON_STOCK_TYPE_CODE = "NON_STOCK";

interface NonStockPartStatusProps {
  received: number;
  planned: number;
}

export function NonStockPartStatus({ received, planned }: NonStockPartStatusProps) {
  const { t } = useTranslation("ot");
  const { formatNumber } = useFormatters();
  return (
    <div className="flex flex-wrap items-center gap-1">
      <StatusChip tone="info" label={t("nonStockPurchase.badge")} />
      {received > 0 ? (
        <StatusChip
          tone={received >= planned ? "success" : "progress"}
          label={t("nonStockPurchase.received", {
            received: formatNumber(received),
            planned: formatNumber(planned),
          })}
        />
      ) : (
        <StatusChip tone="attention" label={t("nonStockPurchase.awaiting")} />
      )}
    </div>
  );
}

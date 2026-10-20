/**
 * "Stock négatif" flag: a negative on-hand or available quantity breaks reservations and reorder
 * points (it can appear when another PC recorded a movement that is received by sync). Shown as
 * a labelled chip, so the warning never relies on colour alone.
 */
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";

const TOLERANCE = 1e-9;

export function isNegativeStock(onHand: number, available: number): boolean {
  return onHand < -TOLERANCE || available < -TOLERANCE;
}

export function NegativeStockBadge({
  onHand,
  available,
  total = false,
}: {
  onHand: number;
  available: number;
  /** The figures are the total over all stores (list): the tooltip says so. */
  total?: boolean;
}) {
  const { t } = useTranslation("inventory");
  if (!isNegativeStock(onHand, available)) return null;
  return (
    <StatusChip
      label={t("negativeStock.label")}
      title={t(total ? "negativeStock.hintTotal" : "negativeStock.hint")}
      tone="danger"
      icon={AlertTriangle}
    />
  );
}

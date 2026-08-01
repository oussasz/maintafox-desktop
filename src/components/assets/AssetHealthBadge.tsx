/**
 * AssetHealthBadge.tsx
 *
 * GAP EQ-07: Health score indicator badge.
 * Composite 0–100 score displayed as colored badge:
 *   80–100 green "Good" | 50–79 amber "Fair" | 0–49 red "Poor" | null gray "No data"
 */

import { Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { getAssetHealthScore } from "@/services/asset-service";
import type { AssetHealthScore } from "@shared/ipc-types";

interface AssetHealthBadgeProps {
  assetId: number;
  /** Compact mode hides the score number, shows only label. */
  compact?: boolean;
}

const LABEL_VARIANT: Record<
  AssetHealthScore["label"],
  { className: string; variant: "default" | "secondary" | "destructive" | "outline" }
> = {
  good: {
    className: "bg-status-success/15 text-status-success border-status-success/30",
    variant: "outline",
  },
  fair: {
    className: "bg-status-warning/15 text-status-warning border-status-warning/30",
    variant: "outline",
  },
  poor: {
    className: "bg-status-danger/15 text-status-danger border-status-danger/30",
    variant: "outline",
  },
  no_data: {
    className: "",
    variant: "secondary",
  },
};

export function AssetHealthBadge({ assetId, compact }: AssetHealthBadgeProps) {
  const { t } = useTranslation("equipment");
  const [health, setHealth] = useState<AssetHealthScore | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getAssetHealthScore(assetId)
      .then((data: AssetHealthScore) => {
        if (!cancelled) setHealth(data);
      })
      .catch(() => {
        if (!cancelled) setHealth(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [assetId]);

  if (loading) {
    return <Loader2 className="h-3 w-3 animate-spin text-text-muted" />;
  }

  if (!health) {
    return (
      <Badge variant="secondary" className="text-[10px]">
        {t("health.unavailable")}
      </Badge>
    );
  }

  const config = LABEL_VARIANT[health.label];
  const labelText =
    health.label === "good"
      ? t("health.good")
      : health.label === "fair"
        ? t("health.fair")
        : health.label === "poor"
          ? t("health.poor")
          : t("health.noData");
  const scoreText = health.score !== null ? `${health.score}` : "";

  return (
    <Badge variant={config.variant} className={`text-[10px] ${config.className}`}>
      {compact ? labelText : `${scoreText} ${labelText}`.trim()}
    </Badge>
  );
}

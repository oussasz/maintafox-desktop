/**
 * AssetHealthBadge.tsx
 *
 * Indicative health score of an asset (0–100, `assets::health`): recent activity, open requests
 * and work orders, meter readings. It is a follow-up indicator, not a measure of availability or
 * condition — the tooltip says what it is made of. A load failure is shown, with its cause.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import type { Tone } from "@/design-system/tone";
import { describeAppError } from "@/lib/app-error";
import { getAssetHealthScore } from "@/services/asset-service";
import type { AssetHealthScore } from "@shared/ipc-types";

interface AssetHealthBadgeProps {
  assetId: number;
  /** Compact mode hides the score number, shows only label. */
  compact?: boolean;
}

const LABEL_TONE: Record<AssetHealthScore["label"], Tone> = {
  good: "success",
  fair: "warning",
  poor: "danger",
  no_data: "neutral",
};

export function AssetHealthBadge({ assetId, compact }: AssetHealthBadgeProps) {
  const { t } = useTranslation("equipment");
  const [health, setHealth] = useState<AssetHealthScore | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    getAssetHealthScore(assetId)
      .then((data: AssetHealthScore) => {
        if (!cancelled) setHealth(data);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setHealth(null);
          setError(err);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [assetId]);

  if (loading) {
    return <StatusChip tone="neutral" label={t("health.loading")} />;
  }

  if (error != null || !health) {
    return (
      <StatusChip
        tone="neutral"
        label={t("health.unavailable")}
        {...(error != null ? { title: describeAppError(error).title } : {})}
      />
    );
  }

  const labelText =
    health.label === "good"
      ? t("health.good")
      : health.label === "fair"
        ? t("health.fair")
        : health.label === "poor"
          ? t("health.poor")
          : t("health.noData");
  // No maintenance evidence yet: "—", never a good-looking default score (L2-06).
  if (health.score === null) {
    return (
      <StatusChip
        tone="neutral"
        label={compact ? labelText : `${t("health.title")} —`}
        title={t("health.noDataHint")}
      />
    );
  }

  return (
    <StatusChip
      tone={LABEL_TONE[health.label]}
      label={compact ? labelText : `${t("health.title")} ${health.score} · ${labelText}`}
      title={t("health.hint")}
    />
  );
}

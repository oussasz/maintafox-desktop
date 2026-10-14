/**
 * CriticalityBadge.tsx
 *
 * Criticality class A–D as a design-system chip (list, detail, tree, pickers).
 */

import { useTranslation } from "react-i18next";

import {
  CRITICALITY_LABEL_KEY,
  CRITICALITY_TONE,
  toCriticalityGrade,
} from "@/components/assets/asset-tones";
import { StatusChip } from "@/components/ui/status-chip";

interface CriticalityBadgeProps {
  criticality: string | null;
  /** Class letter only (dense tables); the full label stays in the tooltip. */
  compact?: boolean;
}

export function CriticalityBadge({ criticality, compact }: CriticalityBadgeProps) {
  const { t } = useTranslation("equipment");
  const grade = toCriticalityGrade(criticality);
  if (!grade) {
    return <span className="text-xs text-text-muted">{t("criticality.notRated")}</span>;
  }
  const label = t(CRITICALITY_LABEL_KEY[grade] as never);
  return (
    <StatusChip
      label={compact ? grade : `${grade} — ${label}`}
      title={`${t("detail.fields.criticality")} ${grade} — ${label}`}
      tone={CRITICALITY_TONE[grade]}
    />
  );
}

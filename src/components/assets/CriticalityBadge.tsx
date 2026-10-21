/**
 * CriticalityBadge.tsx
 *
 * Criticality level as a design-system chip (list, detail, tree, pickers). Tone from the level's
 * rank in EQUIPMENT.CRITICALITY (first = most critical); label translated for the Maintafox
 * levels, else the reference label.
 */

import { useTranslation } from "react-i18next";

import { criticalityLabelKey, criticalityTone } from "@/components/assets/asset-tones";
import { StatusChip } from "@/components/ui/status-chip";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";

interface CriticalityBadgeProps {
  criticality: string | null;
  /** Level code only (dense tables); the full label stays in the tooltip. */
  compact?: boolean;
}

export function CriticalityBadge({ criticality, compact }: CriticalityBadgeProps) {
  const { t } = useTranslation("equipment");
  const { catalog } = useEquipmentTaxonomy();
  const code = criticality?.trim();
  if (!code) {
    return <span className="text-xs text-text-muted">{t("criticality.notRated")}</span>;
  }
  const key = criticalityLabelKey(code);
  const option = catalog?.criticalities.find((c) => c.code.toUpperCase() === code.toUpperCase());
  const label = key ? t(key as never) : (option?.label ?? code);
  return (
    <StatusChip
      label={compact ? code : `${code} — ${label}`}
      title={`${t("detail.fields.criticality")} ${code} — ${label}`}
      tone={criticalityTone(catalog, code)}
    />
  );
}

/**
 * Lifecycle status chip for equipment — shared by list, detail and org views.
 * Tone from the status's availability tag in EQUIPMENT.STATUS; label translated for the
 * Maintafox values, else the reference label (`asset-tones`).
 */

import { useTranslation } from "react-i18next";

import { findStatus, statusLabelKey, statusTone } from "@/components/assets/asset-tones";
import { StatusChip } from "@/components/ui/status-chip";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";

interface AssetStatusBadgeProps {
  code: string;
  /** Larger chip for the detail header. */
  size?: "sm" | "md";
}

export function AssetStatusBadge({ code, size = "sm" }: AssetStatusBadgeProps) {
  const { t } = useTranslation("equipment");
  const { catalog } = useEquipmentTaxonomy();
  const key = statusLabelKey(code);
  const option = findStatus(catalog, code);
  // Until the list arrives (or if it cannot be read) the code stays readable as entered.
  const label = key ? t(key as never) : (option?.label ?? code);
  return <StatusChip label={label} tone={statusTone(catalog, code)} size={size} dot />;
}

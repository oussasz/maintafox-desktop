/**
 * Lifecycle status chip for equipment — shared by list, detail and org views.
 */

import { useTranslation } from "react-i18next";

import {
  ASSET_STATUS_LABEL_KEY,
  ASSET_STATUS_TONE,
  normalizeAssetStatus,
} from "@/components/assets/asset-tones";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";

interface AssetStatusBadgeProps {
  code: string;
  /** Larger chip for the detail header. */
  size?: "sm" | "md";
}

export function AssetStatusBadge({ code, size = "sm" }: AssetStatusBadgeProps) {
  const { t } = useTranslation("equipment");
  const normalized = normalizeAssetStatus(code);
  const key = ASSET_STATUS_LABEL_KEY[normalized];
  // A tenant-defined status without a translation shows its code as entered in the catalogue.
  const label = key ? t(key as never) : code;
  return <StatusChip label={label} tone={toneOf(ASSET_STATUS_TONE, normalized)} size={size} dot />;
}

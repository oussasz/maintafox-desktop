/**
 * UrgencyBadge — chip of a level of the shared DI / OT urgency scale, with the plant's label
 * and, as tooltip, the level's target response / completion times.
 */

import { useTranslation } from "react-i18next";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { StatusChip } from "@/components/ui/status-chip";

export interface UrgencyBadgeProps {
  code: string | null | undefined;
  size?: "sm" | "md";
  className?: string;
}

export function UrgencyBadge({ code, size = "sm", className }: UrgencyBadgeProps) {
  const { t } = useTranslation("common");
  const { labelOf, levelOf } = useUrgencyScale();
  if (!code) return null;
  const level = levelOf(code);
  const title =
    level?.response_target_hours != null && level.completion_target_hours != null
      ? t("urgency.targetsTooltip", {
          response: level.response_target_hours,
          completion: level.completion_target_hours,
        })
      : undefined;
  return (
    <StatusChip
      label={labelOf(code)}
      tone={urgencyTone(code)}
      size={size}
      dot
      {...(title ? { title } : {})}
      {...(className ? { className } : {})}
    />
  );
}

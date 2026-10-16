/**
 * DiSlaStatusBadge.tsx
 *
 * Where a request stands against its SLA: on track / at risk / breached / completed.
 * Colour comes from the shared tone vocabulary (orange only when the request needs attention).
 * Renders nothing when no target applies, so lists stay quiet.
 */

import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { DI_SLA_TONE, toneOf } from "@/design-system/tone";
import type { DiSlaState } from "@shared/ipc-types";

interface DiSlaStatusBadgeProps {
  status: DiSlaState | null | undefined;
  className?: string;
}

export function DiSlaStatusBadge({ status, className }: DiSlaStatusBadgeProps) {
  const { t } = useTranslation("di");
  if (!status) return null;
  return (
    <StatusChip
      label={t(`sla.status.${status}`)}
      tone={toneOf(DI_SLA_TONE, status)}
      dot
      {...(className ? { className } : {})}
    />
  );
}

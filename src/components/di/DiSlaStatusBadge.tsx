/**
 * DiSlaStatusBadge.tsx
 *
 * Where a request stands against its SLA: on track / at risk / breached / completed / paused.
 * A paused clock names why (waiting on the requester, deferred).
 * Colour comes from the shared tone vocabulary (orange only when the request needs attention).
 * Renders nothing when no target applies, so lists stay quiet.
 */

import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { DI_SLA_TONE, toneOf } from "@/design-system/tone";
import type { DiSlaState } from "@shared/ipc-types";

interface DiSlaStatusBadgeProps {
  status: DiSlaState | null | undefined;
  /** Status of the request, to name the reason of a pause. */
  requestStatus?: string;
  className?: string;
}

/** Translation key of the reason a request's clock is paused. */
export function slaPauseReasonKey(
  requestStatus: string | undefined,
): "sla.pausedRequester" | "sla.pausedDeferred" | null {
  if (requestStatus === "returned_for_clarification") return "sla.pausedRequester";
  if (requestStatus === "deferred") return "sla.pausedDeferred";
  return null;
}

export function DiSlaStatusBadge({ status, requestStatus, className }: DiSlaStatusBadgeProps) {
  const { t } = useTranslation("di");
  if (!status) return null;
  const reasonKey = status === "paused" ? slaPauseReasonKey(requestStatus) : null;
  return (
    <StatusChip
      label={reasonKey ? t(reasonKey) : t(`sla.status.${status}`)}
      tone={toneOf(DI_SLA_TONE, status)}
      dot
      {...(className ? { className } : {})}
    />
  );
}

/**
 * DiSlaStatusBadge.tsx
 *
 * Presentational SLA lifecycle badge for DI detail views.
 * Labels: On Track / At Risk / Breached / Completed.
 */

import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import type { DiSlaStatus } from "@shared/ipc-types";

const STATUS_STYLE: Record<string, string> = {
  on_track: "bg-green-100 text-green-800 border-0",
  at_risk: "bg-amber-100 text-amber-800 border-0",
  breached: "bg-red-100 text-red-700 border-0",
  completed: "bg-slate-100 text-slate-700 border-0",
};

interface DiSlaStatusBadgeProps {
  status: DiSlaStatus["status"];
  className?: string;
}

export function DiSlaStatusBadge({ status, className }: DiSlaStatusBadgeProps) {
  const { t } = useTranslation("di");

  if (!status) {
    return (
      <Badge variant="outline" className={className}>
        {t("sla.status.none")}
      </Badge>
    );
  }

  return (
    <Badge variant="outline" className={`${STATUS_STYLE[status] ?? ""} ${className ?? ""}`}>
      {t(`sla.status.${status}`)}
    </Badge>
  );
}

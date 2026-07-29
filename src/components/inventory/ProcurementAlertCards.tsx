/**
 * Procurement dashboard alert tiles — clickable jumps to related tabs.
 */

import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import type { ProcurementAlert } from "@shared/ipc-types";

export type ProcurementAlertTab =
  | "requisitions"
  | "purchase-orders"
  | "repairables"
  | "suppliers"
  | "lifecycle";

export interface ProcurementAlertCardsProps {
  alerts: ProcurementAlert[];
  onNavigate: (tab: ProcurementAlertTab, alert: ProcurementAlert) => void;
  className?: string;
}

function tabForKind(kind: string): ProcurementAlertTab {
  switch (kind) {
    case "LATE_PO":
      return "purchase-orders";
    case "REPAIRABLE_OVERDUE":
      return "repairables";
    case "SUPPLIER_DELAY":
      return "suppliers";
    case "CRITICAL_STOCK":
    default:
      return "requisitions";
  }
}

function severityClass(severity: string): string {
  if (severity === "high") return "border-status-danger/40 bg-status-danger/5";
  if (severity === "medium") return "border-status-warning/40 bg-status-warning/5";
  return "border-surface-border";
}

export function ProcurementAlertCards({
  alerts,
  onNavigate,
  className,
}: ProcurementAlertCardsProps) {
  const { t } = useTranslation("inventory");

  if (alerts.length === 0) return null;

  return (
    <div className={cn("mb-4 space-y-2", className)}>
      <h3 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
        {t("procurement.alerts.title", { defaultValue: "Alerts" })}
      </h3>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
        {alerts.map((alert, index) => (
          <button
            key={`${alert.kind}-${alert.entity_id ?? alert.entity_code ?? index}`}
            type="button"
            onClick={() => onNavigate(tabForKind(alert.kind), alert)}
            className={cn(
              "rounded-md border p-2.5 text-left transition hover:bg-muted/60",
              severityClass(alert.severity),
            )}
          >
            <div className="text-[10px] font-medium uppercase text-text-muted">
              {t(`procurement.alerts.kinds.${alert.kind}`, {
                defaultValue: alert.kind.replaceAll("_", " "),
              })}
            </div>
            <div className="mt-0.5 text-sm font-medium">{alert.title}</div>
            {alert.detail ? (
              <div className="mt-1 text-xs text-text-muted line-clamp-2">{alert.detail}</div>
            ) : null}
          </button>
        ))}
      </div>
    </div>
  );
}

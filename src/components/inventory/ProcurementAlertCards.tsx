/**
 * Procurement dashboard alert tiles — clickable jumps to related tabs.
 *
 * Alerts are coded by the backend (`kind` + `params`); the text is written here in the UI
 * language, quantities and dates through the shared formatters.
 */

import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { type Tone, mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
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

const KNOWN_KINDS = new Set(["CRITICAL_STOCK", "LATE_PO", "REPAIRABLE_OVERDUE", "SUPPLIER_DELAY"]);

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

function severityTone(severity: string): Tone {
  if (severity === "high") return "danger";
  if (severity === "medium") return "warning";
  return "neutral";
}

export function ProcurementAlertCards({
  alerts,
  onNavigate,
  className,
}: ProcurementAlertCardsProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatDate } = useFormatters();

  if (alerts.length === 0) return null;

  const num = (v: string | undefined) => {
    const n = v != null ? Number(v) : Number.NaN;
    return Number.isFinite(n) ? formatNumber(n) : "—";
  };

  const message = (alert: ProcurementAlert): string => {
    const p = alert.params;
    const code = alert.entity_code ?? "—";
    switch (alert.kind) {
      case "CRITICAL_STOCK":
        return t("procurement.alerts.messages.CRITICAL_STOCK", {
          code,
          name: p["name"] ?? "",
          warehouse: p["warehouse_code"] ?? "",
          available: num(p["available_qty"]),
          threshold: num(p["threshold_qty"]),
        });
      case "LATE_PO":
        return t("procurement.alerts.messages.LATE_PO", {
          code,
          date: p["expected_date"] ? formatDate(p["expected_date"]) : "—",
        });
      case "REPAIRABLE_OVERDUE":
        return t("procurement.alerts.messages.REPAIRABLE_OVERDUE", {
          code,
          article: p["article_code"] ?? "",
          days: num(p["lead_days"]),
        });
      case "SUPPLIER_DELAY":
        return t("procurement.alerts.messages.SUPPLIER_DELAY", { code, name: p["name"] ?? "" });
      default:
        return code;
    }
  };

  return (
    <div className={cn("mb-4 space-y-2", className)}>
      <h3 className="text-xs font-semibold uppercase tracking-wide text-text-muted">
        {t("procurement.alerts.title")}
      </h3>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
        {alerts.map((alert, index) => {
          const tone = severityTone(alert.severity);
          return (
            <button
              key={`${alert.kind}-${alert.entity_id ?? alert.entity_code ?? index}`}
              type="button"
              onClick={() => onNavigate(tabForKind(alert.kind), alert)}
              className={cn(
                "min-h-[44px] rounded-md border p-2.5 text-left transition hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                mfTone[tone].panel,
              )}
            >
              <StatusChip
                label={
                  KNOWN_KINDS.has(alert.kind)
                    ? t(`procurement.alerts.kinds.${alert.kind}`)
                    : t("procurement.alerts.kinds.other")
                }
                tone={tone}
              />
              <div className="mt-1 text-sm text-text-primary">{message(alert)}</div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

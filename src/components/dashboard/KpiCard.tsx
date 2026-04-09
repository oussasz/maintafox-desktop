import { ArrowDown, ArrowUp, Minus } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";

export interface KpiCardProps {
  title: string;
  value: number;
  trend: number;
  icon: LucideIcon;
  color: string;
  /** Whether an upward trend is good or bad. */
  trendDirection: "up-good" | "up-bad";
  /** If false, the KPI data source is not yet available (Phase 5). */
  available?: boolean;
}

export function KpiCard({
  title,
  value,
  trend,
  icon: Icon,
  color,
  trendDirection,
  available = true,
}: KpiCardProps) {
  const { t } = useTranslation("dashboard");

  const trendIsPositive = trend > 0;
  const trendIsNeutral = trend === 0;

  // Determine color based on direction semantics
  const trendColorClass = trendIsNeutral
    ? "text-text-muted"
    : trendDirection === "up-good"
      ? trendIsPositive
        ? "text-status-success"
        : "text-status-danger"
      : trendIsPositive
        ? "text-status-danger"
        : "text-status-success";

  const TrendIcon = trendIsNeutral ? Minus : trendIsPositive ? ArrowUp : ArrowDown;

  return (
    <Card className="relative overflow-hidden">
      <CardContent className="p-4">
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium text-text-muted">{title}</p>
          <div
            className="flex h-8 w-8 items-center justify-center rounded-lg"
            style={{ backgroundColor: `${color}15` }}
          >
            <Icon className="h-4 w-4" style={{ color }} />
          </div>
        </div>

        <div className="mt-2">
          {available ? (
            <p className="text-3xl font-bold text-text-primary tabular-nums">
              {value.toLocaleString()}
            </p>
          ) : (
            <p className="text-3xl font-bold text-text-muted" title={t("kpi.notAvailableYet")}>
              —
            </p>
          )}
        </div>

        {available && (
          <div className={cn("mt-1 flex items-center gap-1 text-xs", trendColorClass)}>
            <TrendIcon className="h-3 w-3" />
            <span>
              {trendIsNeutral ? t("kpi.noChange") : `${Math.abs(trend)} ${t("kpi.vsPrevious")}`}
            </span>
          </div>
        )}

        {!available && <p className="mt-1 text-xs text-text-muted">{t("kpi.comingSoon")}</p>}
      </CardContent>
    </Card>
  );
}

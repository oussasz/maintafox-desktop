import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { CHART_COLORS } from "@/components/charts/chart-utils";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { AnalyticsDiStatusSegment } from "@shared/ipc-types";

const COLORS = [
  CHART_COLORS.primary,
  CHART_COLORS.accent,
  CHART_COLORS.success,
  CHART_COLORS.warning,
  CHART_COLORS.danger,
  CHART_COLORS.neutral,
];

export function AnalyticsDiStatusChart(props: { segments: AnalyticsDiStatusSegment[] }) {
  const { t } = useTranslation("dashboard");
  const { segments } = props;
  const max = useMemo(() => Math.max(1, ...segments.map((s) => s.count)), [segments]);

  if (segments.length === 0) {
    return null;
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-base font-medium">{t("widgets.diStatus.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        {segments.map((seg, i) => (
          <div key={seg.status} className="space-y-1">
            <div className="flex justify-between text-xs">
              <span className="font-medium text-text-primary">{seg.status}</span>
              <span className="tabular-nums text-text-muted">{seg.count}</span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-surface-2">
              <div
                className="h-full rounded-full transition-all"
                style={{
                  width: `${(seg.count / max) * 100}%`,
                  backgroundColor: COLORS[i % COLORS.length],
                }}
              />
            </div>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

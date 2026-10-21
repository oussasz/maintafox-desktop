import type { TFunction } from "i18next";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { DI_WORKFLOW_STATUS_TONE, mfTone, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import type { AnalyticsDiStatusSegment } from "@shared/ipc-types";

/** Translated label for a DI workflow status code; unknown codes read "Other status". */
export function diStatusLabel(t: TFunction<"reports">, code: string): string {
  return t(`hub.dashboards.diStatus.${code}`, {
    defaultValue: t("hub.dashboards.diStatus.other"),
  });
}

export function AnalyticsDiStatusChart(props: { segments: AnalyticsDiStatusSegment[] }) {
  const { t } = useTranslation("reports");
  const navigate = useNavigate();
  const { formatNumber } = useFormatters();
  const { segments } = props;
  const max = useMemo(() => Math.max(1, ...segments.map((s) => s.count)), [segments]);
  const total = useMemo(() => segments.reduce((sum, s) => sum + s.count, 0), [segments]);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-base font-medium">
          {t("hub.dashboards.widgets.diStatus")}
        </CardTitle>
        <Button
          type="button"
          variant="link"
          className="h-10 px-2 text-xs"
          onClick={() => navigate("/requests")}
        >
          {t("hub.dashboards.diStatus.openList")}
        </Button>
      </CardHeader>
      <CardContent className="space-y-2">
        {segments.length === 0 ? (
          <p className="text-sm text-text-muted">{t("hub.dashboards.diStatus.empty")}</p>
        ) : (
          <ul className="space-y-2" aria-label={t("hub.dashboards.diStatus.aria", { total })}>
            {segments.map((seg) => {
              const label = diStatusLabel(t, seg.status);
              const tone = toneOf(DI_WORKFLOW_STATUS_TONE, seg.status);
              return (
                <li key={seg.status} className="space-y-1">
                  <div className="flex justify-between text-xs">
                    <span className="font-medium text-text-primary">{label}</span>
                    <span className="tabular-nums text-text-muted">{formatNumber(seg.count)}</span>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-surface-2" aria-hidden>
                    <div
                      className={cn("h-full rounded-full transition-all", mfTone[tone].bar)}
                      style={{ width: `${(seg.count / max) * 100}%` }}
                    />
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

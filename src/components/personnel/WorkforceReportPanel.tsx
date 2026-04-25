import { BarChart3, Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import {
  getWorkforceKpiReport,
  getWorkforceSkillsGapReport,
  getWorkforceSummaryReport,
} from "@/services/personnel-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkforceKpiReport, WorkforceSkillsGapRow, WorkforceSummaryReport } from "@shared/ipc-types";

export function WorkforceReportPanel() {
  const { t } = useTranslation("personnel");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [summary, setSummary] = useState<WorkforceSummaryReport | null>(null);
  const [kpi, setKpi] = useState<WorkforceKpiReport | null>(null);
  const [skillsGap, setSkillsGap] = useState<WorkforceSkillsGapRow[]>([]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [nextSummary, nextKpi, nextSkills] = await Promise.all([
        getWorkforceSummaryReport(),
        getWorkforceKpiReport(),
        getWorkforceSkillsGapReport(),
      ]);
      setSummary(nextSummary);
      setKpi(nextKpi);
      setSkillsGap(nextSkills.slice(0, 8));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <PermissionGate permission="per.report">
      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <div>
            <CardTitle className="flex items-center gap-2">
              <BarChart3 className="h-4 w-4" />
              {t("reports.panel.title")}
            </CardTitle>
            <CardDescription>{t("reports.panel.description")}</CardDescription>
          </div>
          <Button size="sm" variant="outline" onClick={() => void refresh()} disabled={loading}>
            {loading ? <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" /> : <RefreshCw className="mr-2 h-3.5 w-3.5" />}
            {t("reports.panel.refresh")}
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          {error ? <div className="text-sm text-destructive">{error}</div> : null}

          <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
            <Badge variant="secondary" className="justify-center py-2">
              {t("reports.kpis.totalPersonnel", { count: summary?.total_personnel ?? 0 })}
            </Badge>
            <Badge variant="secondary" className="justify-center py-2">
              {t("reports.kpis.avgSkills", { value: kpi?.avg_skills_per_person ?? 0 })}
            </Badge>
            <Badge variant="secondary" className="justify-center py-2">
              {t("reports.kpis.blockedRatio", { value: kpi?.blocked_ratio ?? 0 })}
            </Badge>
          </div>

          <Separator />

          <div className="space-y-2">
            <div className="text-sm font-medium">{t("reports.skillsGap.title")}</div>
            {skillsGap.length === 0 ? (
              <div className="text-sm text-muted-foreground">{t("reports.skillsGap.empty")}</div>
            ) : (
              <div className="space-y-1">
                {skillsGap.map((row) => (
                  <div key={row.personnel_id} className="flex items-center justify-between rounded border px-2 py-1 text-xs">
                    <span className="font-mono">{row.employee_code}</span>
                    <span className="truncate px-2">{row.full_name}</span>
                    <Badge variant="outline">{t("reports.skillsGap.score", { score: row.gap_score })}</Badge>
                  </div>
                ))}
              </div>
            )}
          </div>
        </CardContent>
      </Card>
    </PermissionGate>
  );
}

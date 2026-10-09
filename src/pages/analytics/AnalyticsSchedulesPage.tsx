import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { usePermissions } from "@/hooks/use-permissions";
import {
  deleteMyReportSchedule,
  listMyReportSchedules,
  listReportTemplates,
  upsertMyReportSchedule,
} from "@/services/reports-service";
import type { ReportSchedule, ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function AnalyticsSchedulesPage() {
  const { t } = useTranslation("reports");
  const { can } = usePermissions();
  const canManage = can(P.REP_MANAGE);

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [schedules, setSchedules] = useState<ReportSchedule[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [templateId, setTemplateId] = useState(1);
  const [cronExpr, setCronExpr] = useState("0 0 8 * * *");
  const [exportFormat, setExportFormat] = useState<"pdf" | "xlsx">("pdf");
  const [enabled, setEnabled] = useState(true);

  const templateById = useMemo(() => {
    const m = new Map<number, ReportTemplate>();
    for (const x of templates) {
      m.set(x.id, x);
    }
    return m;
  }, [templates]);

  const load = useCallback(() => {
    if (!canManage) {
      return;
    }
    setError(null);
    Promise.all([listReportTemplates(), listMyReportSchedules()])
      .then(([tpl, sched]) => {
        setTemplates(tpl);
        setSchedules(sched);
        setTemplateId((current) => {
          if (tpl.length === 0) {
            return current;
          }
          const first = tpl[0];
          if (!first) {
            return current;
          }
          return tpl.some((x) => x.id === current) ? current : first.id;
        });
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [canManage]);

  useEffect(() => {
    load();
  }, [load]);

  const onSaveSchedule = async () => {
    if (!canManage) {
      return;
    }
    try {
      await upsertMyReportSchedule({
        template_id: templateId,
        cron_expr: cronExpr.trim(),
        export_format: exportFormat,
        enabled,
      });
      load();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const onDeleteSchedule = async (id: number) => {
    if (!canManage) {
      return;
    }
    try {
      await deleteMyReportSchedule(id);
      load();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (!canManage) {
    return <p className="text-sm text-text-muted">{t("schedule.noAccess")}</p>;
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("schedule.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {error ? <p className="text-sm text-text-danger">{error}</p> : null}
        <p className="text-xs text-text-muted">{t("schedule.cronHint")}</p>
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1 text-xs">
            <span>{t("schedule.template")}</span>
            <select
              className="rounded border border-surface-border bg-surface-0 px-2 py-1 text-sm"
              value={templateId}
              onChange={(e) => setTemplateId(Number(e.target.value))}
            >
              {templates.map((x) => (
                <option key={x.code} value={x.id}>
                  {x.title} ({x.code})
                </option>
              ))}
            </select>
          </label>
          <label className="flex min-w-[12rem] flex-col gap-1 text-xs">
            <span>{t("schedule.cron")}</span>
            <input
              className="rounded border border-surface-border bg-surface-0 px-2 py-1 font-mono text-sm"
              value={cronExpr}
              onChange={(e) => setCronExpr(e.target.value)}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            <span>{t("schedule.format")}</span>
            <select
              className="rounded border border-surface-border bg-surface-0 px-2 py-1 text-sm"
              value={exportFormat}
              onChange={(e) => setExportFormat(e.target.value as "pdf" | "xlsx")}
            >
              <option value="pdf">{t("format.pdf")}</option>
              <option value="xlsx">{t("format.xlsx")}</option>
            </select>
          </label>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />
            {t("schedule.enabled")}
          </label>
          <Button type="button" size="sm" onClick={() => void onSaveSchedule()}>
            {t("schedule.save")}
          </Button>
        </div>

        <table className="mt-4 w-full border-collapse text-left text-sm">
          <thead>
            <tr className="border-b border-surface-border text-text-muted">
              <th className="py-2 pr-2">{t("schedule.template")}</th>
              <th className="py-2 pr-2">{t("schedule.cron")}</th>
              <th className="py-2 pr-2">{t("schedule.next")}</th>
              <th className="py-2" />
            </tr>
          </thead>
          <tbody>
            {schedules.map((s) => {
              const tpl = templateById.get(s.template_id);
              return (
                <tr
                  key={`${s.template_id}-${s.cron_expr}-${s.next_run_at}`}
                  className="border-b border-surface-border/60"
                >
                  <td className="py-2 pr-2">
                    {tpl ? `${tpl.title} (${tpl.code})` : t("common.empty")}
                  </td>
                  <td className="py-2 pr-2 font-mono text-xs">{s.cron_expr}</td>
                  <td className="py-2 pr-2 text-xs">{s.next_run_at}</td>
                  <td className="py-2">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => void onDeleteSchedule(s.id)}
                    >
                      {t("schedule.delete")}
                    </Button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </CardContent>
    </Card>
  );
}

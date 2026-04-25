import { BarChart3 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { usePermissions } from "@/hooks/use-permissions";
import {
  deleteMyReportSchedule,
  downloadExportedDocument,
  exportReportNow,
  listMyReportRuns,
  listMyReportSchedules,
  listReportTemplates,
  upsertMyReportSchedule,
} from "@/services/reports-service";
import type { ReportRun, ReportSchedule, ReportTemplate } from "@shared/ipc-types";

export function AnalyticsPage() {
  const { t } = useTranslation("reports");
  const { can } = usePermissions();
  const canView = can("rep.view");
  const canExport = can("rep.export");
  const canManage = can("rep.manage");

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [schedules, setSchedules] = useState<ReportSchedule[]>([]);
  const [runs, setRuns] = useState<ReportRun[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [templateId, setTemplateId] = useState(1);
  const [cronExpr, setCronExpr] = useState("0 0 8 * * *");
  const [exportFormat, setExportFormat] = useState<"pdf" | "xlsx">("pdf");
  const [enabled, setEnabled] = useState(true);

  const load = useCallback(() => {
    if (!canView) {
      return;
    }
    setError(null);
    Promise.all([listReportTemplates(), listMyReportSchedules(), listMyReportRuns(40)])
      .then(([t, s, r]) => {
        setTemplates(t);
        setSchedules(s);
        setRuns(r);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [canView]);

  useEffect(() => {
    load();
  }, [load]);

  const onExport = async (code: string, fmt: "pdf" | "xlsx") => {
    if (!canExport) {
      return;
    }
    try {
      const doc = await exportReportNow({ template_code: code, export_format: fmt });
      downloadExportedDocument(doc);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

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

  const fmtLabel = (code: string) => t(`format.${code}`, { defaultValue: code });

  const runStatusLabel = (code: string) => t(`runs.statusValues.${code}`, { defaultValue: code });

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

  if (!canView) {
    return (
      <ModulePageShell icon={BarChart3} title={t("page.title")} bodyClassName="p-4">
        <p className="text-sm text-text-muted">{t("page.noAccess")}</p>
      </ModulePageShell>
    );
  }

  return (
    <ModulePageShell
      icon={BarChart3}
      title={t("page.title")}
      description={t("page.subtitle")}
      bodyClassName="space-y-6 p-4"
    >
      {error ? <p className="text-sm text-text-danger">{error}</p> : null}

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("templates.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          <table className="w-full border-collapse text-left text-sm">
            <thead>
              <tr className="border-b border-surface-border text-text-muted">
                <th className="py-2 pr-2">{t("templates.code")}</th>
                <th className="py-2 pr-2">{t("templates.name")}</th>
                <th className="py-2 pr-2">{t("templates.defaultFormat")}</th>
                <th className="py-2">{t("templates.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {templates.map((x) => (
                <tr key={x.id} className="border-b border-surface-border/60">
                  <td className="py-2 pr-2 font-mono text-xs">{x.code}</td>
                  <td className="py-2 pr-2">{x.title}</td>
                  <td className="py-2 pr-2">{fmtLabel(x.default_format)}</td>
                  <td className="py-2">
                    {canExport ? (
                      <div className="flex flex-wrap gap-2">
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => void onExport(x.code, "pdf")}
                        >
                          {t("format.pdf")}
                        </Button>
                        <Button
                          type="button"
                          variant="outline"
                          size="sm"
                          onClick={() => void onExport(x.code, "xlsx")}
                        >
                          {t("format.xlsx")}
                        </Button>
                      </div>
                    ) : (
                      <span className="text-xs text-text-muted">{t("common.empty")}</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </CardContent>
      </Card>

      {canManage ? (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t("schedule.title")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
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
                    <option key={x.id} value={x.id}>
                      {x.code}
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
                  <th className="py-2 pr-2">{t("table.id")}</th>
                  <th className="py-2 pr-2">{t("schedule.template")}</th>
                  <th className="py-2 pr-2">{t("schedule.cron")}</th>
                  <th className="py-2 pr-2">{t("schedule.next")}</th>
                  <th className="py-2" />
                </tr>
              </thead>
              <tbody>
                {schedules.map((s) => (
                  <tr key={s.id} className="border-b border-surface-border/60">
                    <td className="py-2 pr-2 font-mono text-xs">{s.id}</td>
                    <td className="py-2 pr-2">{s.template_id}</td>
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
                ))}
              </tbody>
            </table>
          </CardContent>
        </Card>
      ) : null}

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("runs.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          <table className="w-full border-collapse text-left text-sm">
            <thead>
              <tr className="border-b border-surface-border text-text-muted">
                <th className="py-2 pr-2">{t("table.id")}</th>
                <th className="py-2 pr-2">{t("runs.status")}</th>
                <th className="py-2 pr-2">{t("runs.format")}</th>
                <th className="py-2 pr-2">{t("runs.started")}</th>
                <th className="py-2 pr-2">{t("runs.path")}</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((r) => (
                <tr key={r.id} className="border-b border-surface-border/60">
                  <td className="py-2 pr-2 font-mono text-xs">{r.id}</td>
                  <td className="py-2 pr-2">{runStatusLabel(r.status)}</td>
                  <td className="py-2 pr-2">{fmtLabel(r.export_format)}</td>
                  <td className="py-2 pr-2 text-xs">{r.started_at}</td>
                  <td
                    className="max-w-[14rem] truncate py-2 pr-2 font-mono text-[10px]"
                    title={r.artifact_path ?? ""}
                  >
                    {r.error_message ?? r.artifact_path ?? t("common.empty")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </CardContent>
      </Card>
    </ModulePageShell>
  );
}

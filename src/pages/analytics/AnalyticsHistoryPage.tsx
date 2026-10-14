import { Download, History } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { SavedReportNotice, useReportFileSave } from "@/components/analytics/report-file-save";
import {
  reportFormatLabel,
  reportTemplateTitle,
} from "@/components/analytics/report-template-label";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { REPORT_RUN_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  downloadReportRunArtifact,
  listMyReportRuns,
  listReportTemplates,
} from "@/services/reports-service";
import type { ReportRun, ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Most recent runs shown; the backend returns them newest first. */
const RUN_LIMIT = 50;

export function AnalyticsHistoryPage() {
  const { t } = useTranslation("reports");
  const { formatDateTime } = useFormatters();
  const { can } = usePermissions();
  const canExport = can(P.REP_EXPORT);
  const { save, savedPath, clearSaved } = useReportFileSave();

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [runs, setRuns] = useState<ReportRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [downloadingRunId, setDownloadingRunId] = useState<number | null>(null);

  const templateById = useMemo(() => new Map(templates.map((x) => [x.id, x])), [templates]);

  const load = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    Promise.all([listReportTemplates(), listMyReportRuns(RUN_LIMIT)])
      .then(([tpl, r]) => {
        setTemplates(tpl);
        setRuns(r);
      })
      .catch((e: unknown) => setLoadError(e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const onDownload = async (run: ReportRun) => {
    if (!canExport || !run.has_downloadable_artifact) return;
    setDownloadingRunId(run.id);
    clearSaved();
    try {
      await save(() => downloadReportRunArtifact(run.id));
    } finally {
      setDownloadingRunId(null);
    }
  };

  const templateName = (id: number) => {
    const tpl = templateById.get(id);
    return tpl ? reportTemplateTitle(t, tpl) : t("runs.unknownTemplate");
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("runs.title")}</CardTitle>
        <p className="text-xs text-text-muted">{t("runs.subtitle", { count: RUN_LIMIT })}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        {savedPath ? <SavedReportNotice path={savedPath} onDismiss={clearSaved} /> : null}

        {loading && runs.length === 0 ? (
          <LoadingState label={t("runs.loading")} />
        ) : loadError ? (
          <ErrorState error={loadError} title={t("runs.loadError")} onRetry={load} />
        ) : runs.length === 0 ? (
          <EmptyState
            icon={<History className="h-6 w-6" aria-hidden />}
            title={t("runs.empty")}
            description={t("runs.emptyHint")}
          />
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-left text-sm">
              <thead>
                <tr className="border-b border-surface-border text-xs text-text-muted">
                  <th className="py-2 pr-3 font-medium">{t("runs.report")}</th>
                  <th className="py-2 pr-3 font-medium">{t("runs.origin")}</th>
                  <th className="py-2 pr-3 font-medium">{t("runs.status")}</th>
                  <th className="py-2 pr-3 font-medium">{t("runs.format")}</th>
                  <th className="py-2 pr-3 font-medium">{t("runs.started")}</th>
                  <th className="py-2 font-medium">
                    <span className="sr-only">{t("runs.download")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {runs.map((r) => {
                  const downloadable =
                    canExport && r.has_downloadable_artifact && r.status === "success";
                  return (
                    <tr key={r.id} className="border-b border-surface-border/60 align-top">
                      <td className="py-2 pr-3 text-text-primary">{templateName(r.template_id)}</td>
                      <td className="py-2 pr-3 text-xs text-text-muted">
                        {r.schedule_id != null ? t("runs.originScheduled") : t("runs.originManual")}
                      </td>
                      <td className="py-2 pr-3">
                        <StatusChip
                          label={t(`runs.statusValues.${r.status}`, {
                            defaultValue: t("runs.statusValues.unknown"),
                          })}
                          tone={toneOf(REPORT_RUN_STATUS_TONE, r.status)}
                        />
                        {r.status === "success" && r.has_downloadable_artifact ? (
                          <p className="mt-1 text-xs text-text-muted">{t("runs.saved")}</p>
                        ) : null}
                        {r.error_message ? (
                          <details className="mt-1 max-w-xs text-xs text-text-muted">
                            <summary className="cursor-pointer select-none">
                              {t("runs.technicalDetails")}
                            </summary>
                            <p className="mt-1 whitespace-pre-wrap break-words">
                              {r.error_message}
                            </p>
                          </details>
                        ) : null}
                      </td>
                      <td className="py-2 pr-3">{reportFormatLabel(t, r.export_format)}</td>
                      <td className="whitespace-nowrap py-2 pr-3 text-xs">
                        {formatDateTime(r.started_at)}
                      </td>
                      <td className="py-2 text-right">
                        {downloadable ? (
                          <Button
                            type="button"
                            variant="outline"
                            className="h-10"
                            disabled={downloadingRunId !== null}
                            onClick={() => void onDownload(r)}
                          >
                            <Download className="mr-1.5 h-4 w-4" aria-hidden />
                            {downloadingRunId === r.id ? t("runs.downloading") : t("runs.download")}
                          </Button>
                        ) : null}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { usePermissions } from "@/hooks/use-permissions";
import {
  downloadExportedDocument,
  downloadReportRunArtifact,
  listMyReportRuns,
  listReportTemplates,
} from "@/services/reports-service";
import type { ReportRun, ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function AnalyticsHistoryPage() {
  const { t } = useTranslation("reports");
  const { can } = usePermissions();
  const canExport = can(P.REP_EXPORT);

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [runs, setRuns] = useState<ReportRun[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [downloadingKey, setDownloadingKey] = useState<string | null>(null);

  const templateById = useMemo(() => {
    const m = new Map<number, ReportTemplate>();
    for (const x of templates) {
      m.set(x.id, x);
    }
    return m;
  }, [templates]);

  const load = useCallback(() => {
    setError(null);
    Promise.all([listReportTemplates(), listMyReportRuns(40)])
      .then(([tpl, r]) => {
        setTemplates(tpl);
        setRuns(r);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const fmtLabel = (code: string) => t(`format.${code}`, { defaultValue: code });
  const runStatusLabel = (code: string) => t(`runs.statusValues.${code}`, { defaultValue: code });

  const onDownload = async (run: ReportRun) => {
    if (!canExport || !run.has_downloadable_artifact) {
      return;
    }
    const key = `${run.started_at}-${run.template_id}-${run.export_format}`;
    setDownloadingKey(key);
    setError(null);
    try {
      const doc = await downloadReportRunArtifact(run.id);
      downloadExportedDocument(doc);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDownloadingKey(null);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("runs.title")}</CardTitle>
      </CardHeader>
      <CardContent>
        {error ? <p className="mb-3 text-sm text-text-danger">{error}</p> : null}
        <table className="w-full border-collapse text-left text-sm">
          <thead>
            <tr className="border-b border-surface-border text-text-muted">
              <th className="py-2 pr-2">{t("schedule.template")}</th>
              <th className="py-2 pr-2">{t("runs.status")}</th>
              <th className="py-2 pr-2">{t("runs.format")}</th>
              <th className="py-2 pr-2">{t("runs.started")}</th>
              <th className="py-2 pr-2">{t("runs.detail")}</th>
              <th className="py-2">{t("runs.download")}</th>
            </tr>
          </thead>
          <tbody>
            {runs.map((r) => {
              const tpl = templateById.get(r.template_id);
              const rowKey = `${r.started_at}-${r.template_id}-${r.export_format}-${r.status}`;
              return (
                <tr key={rowKey} className="border-b border-surface-border/60">
                  <td className="py-2 pr-2">
                    {tpl ? `${tpl.title} (${tpl.code})` : t("common.empty")}
                  </td>
                  <td className="py-2 pr-2">{runStatusLabel(r.status)}</td>
                  <td className="py-2 pr-2">{fmtLabel(r.export_format)}</td>
                  <td className="py-2 pr-2 text-xs">{r.started_at}</td>
                  <td
                    className="max-w-[14rem] truncate py-2 pr-2 font-mono text-[10px]"
                    title={r.error_message ?? r.artifact_path ?? ""}
                  >
                    {r.error_message ??
                      (r.has_downloadable_artifact ? t("runs.archived") : t("common.empty"))}
                  </td>
                  <td className="py-2">
                    {canExport && r.has_downloadable_artifact && r.status === "success" ? (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={downloadingKey === rowKey}
                        onClick={() => void onDownload(r)}
                      >
                        {t("runs.download")}
                      </Button>
                    ) : (
                      <span className="text-xs text-text-muted">{t("common.empty")}</span>
                    )}
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

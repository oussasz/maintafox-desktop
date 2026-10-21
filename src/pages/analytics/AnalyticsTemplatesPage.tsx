import { FileSpreadsheet, FileStack, FileText } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { SavedReportNotice, useReportFileSave } from "@/components/analytics/report-file-save";
import {
  reportFormatLabel,
  reportTemplateDescription,
  reportTemplateTitle,
} from "@/components/analytics/report-template-label";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { exportReportNow, listReportTemplates, toReportLocale } from "@/services/reports-service";
import type { ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ExportFormat = "pdf" | "xlsx";

export function AnalyticsTemplatesPage() {
  const { t } = useTranslation("reports");
  const { locale } = useFormatters();
  const { can } = usePermissions();
  const canExport = can(P.REP_EXPORT);
  const { save, savedPath, clearSaved } = useReportFileSave();

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    listReportTemplates()
      .then(setTemplates)
      .catch((e: unknown) => setLoadError(e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const onExport = async (code: string, fmt: ExportFormat) => {
    if (!canExport) return;
    setBusy(`${code}:${fmt}`);
    clearSaved();
    try {
      await save(() =>
        exportReportNow({
          template_code: code,
          export_format: fmt,
          locale: toReportLocale(locale),
        }),
      );
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("templates.title")}</CardTitle>
        <p className="text-xs text-text-muted">{t("templates.subtitle")}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        {savedPath ? <SavedReportNotice path={savedPath} onDismiss={clearSaved} /> : null}
        {!canExport ? <p className="text-xs text-text-muted">{t("templates.noExport")}</p> : null}

        {loading && templates.length === 0 ? (
          <LoadingState label={t("templates.loading")} />
        ) : loadError ? (
          <ErrorState error={loadError} title={t("templates.loadError")} onRetry={load} />
        ) : templates.length === 0 ? (
          <EmptyState
            icon={<FileStack className="h-6 w-6" aria-hidden />}
            title={t("templates.empty")}
            description={t("templates.emptyHint")}
          />
        ) : (
          <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
            {templates.map((x) => {
              const title = reportTemplateTitle(t, x);
              return (
                <li
                  key={x.code}
                  className="flex flex-wrap items-center justify-between gap-3 px-3 py-3"
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-text-primary">{title}</p>
                    <p className="text-xs text-text-muted">{reportTemplateDescription(t, x)}</p>
                    <p className="mt-0.5 text-xs text-text-muted">
                      {t("templates.defaultFormat")}: {reportFormatLabel(t, x.default_format)}
                    </p>
                  </div>
                  {canExport ? (
                    <div className="flex flex-wrap gap-2">
                      {(["pdf", "xlsx"] as const).map((fmt) => (
                        <Button
                          key={fmt}
                          type="button"
                          variant="outline"
                          className="h-10"
                          disabled={busy !== null}
                          aria-label={t("templates.exportAria", {
                            title,
                            format: reportFormatLabel(t, fmt),
                          })}
                          onClick={() => void onExport(x.code, fmt)}
                        >
                          {fmt === "pdf" ? (
                            <FileText className="mr-1.5 h-4 w-4" aria-hidden />
                          ) : (
                            <FileSpreadsheet className="mr-1.5 h-4 w-4" aria-hidden />
                          )}
                          {busy === `${x.code}:${fmt}`
                            ? t("templates.exporting")
                            : reportFormatLabel(t, fmt)}
                        </Button>
                      ))}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

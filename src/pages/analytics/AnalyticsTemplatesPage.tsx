import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { usePermissions } from "@/hooks/use-permissions";
import {
  downloadExportedDocument,
  exportReportNow,
  listReportTemplates,
} from "@/services/reports-service";
import type { ReportTemplate } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function AnalyticsTemplatesPage() {
  const { t } = useTranslation("reports");
  const { can } = usePermissions();
  const canExport = can(P.REP_EXPORT);

  const [templates, setTemplates] = useState<ReportTemplate[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    listReportTemplates()
      .then(setTemplates)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

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
      load();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const fmtLabel = (code: string) => t(`format.${code}`, { defaultValue: code });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t("templates.title")}</CardTitle>
      </CardHeader>
      <CardContent>
        {error ? <p className="mb-3 text-sm text-text-danger">{error}</p> : null}
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
              <tr key={x.code} className="border-b border-surface-border/60">
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
  );
}

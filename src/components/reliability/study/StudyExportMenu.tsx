/**
 * Export of a reliability study, or of its decision note for management (L2-14), through the
 * app's shared export transport: print (the print dialog also saves as PDF) and CSV for Excel,
 * both written through `src/export`. The figures are read again when the button is pressed: the
 * study's saved costs and plan, or the analysis on screen for the note.
 */
import { Download, FileSpreadsheet, Printer } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { isExportCancelled, printHtml } from "@/export";
import { saveCsvWithDialog } from "@/export/csv";
import { loadDocumentHeaderHtml } from "@/export/templates/document-header";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import {
  type LifeAnalysis,
  type ReliabilityStudy,
  type StudyDecision,
  getReliabilityStudyLifeAnalysis,
  listReliabilityStudyDecisions,
} from "@/services/reliability-study-service";
import { pushAppToast } from "@/store/app-toast-store";

import {
  type StudyReport,
  type StudyReportScope,
  buildStudyReport,
  studyReportCsvRows,
  studyReportHtml,
} from "./study-report";

export function StudyExportMenu({
  study,
  scope,
  analysis = null,
}: {
  study: ReliabilityStudy;
  scope: StudyReportScope;
  /** The analysis shown on screen (the note exports what is read); fetched when absent. */
  analysis?: LifeAnalysis | null;
}) {
  const { t, i18n } = useTranslation("reliability");
  const { formatNumber, formatDecimal, formatCurrency, formatDate, formatDateTime, currencyCode } =
    useFormatters();
  const [busy, setBusy] = useState(false);

  const build = async (): Promise<StudyReport> => {
    const [a, decisions] = await Promise.all([
      analysis
        ? Promise.resolve(analysis)
        : getReliabilityStudyLifeAnalysis({
            study_id: study.id,
            planned_cost: null,
            failure_cost: null,
            pm_plan_id: null,
          }),
      scope === "study"
        ? listReliabilityStudyDecisions(study.id)
        : Promise.resolve([] as StudyDecision[]),
    ]);
    const currency = a.cost_currency_code ?? currencyCode;
    return buildStudyReport(scope, study, a, decisions, {
      t: (key, opts) => (t as (k: string, o?: Record<string, unknown>) => string)(key, opts),
      number: formatNumber,
      decimal: formatDecimal,
      money: (v) => formatCurrency(v, currency),
      date: formatDate,
    });
  };

  const fail = (err: unknown) => {
    if (isExportCancelled(err)) return;
    const { title, description } = describeAppError(err);
    pushAppToast({
      title: t("rams.export.failed"),
      description: description ? `${title} — ${description}` : title,
      variant: "destructive",
    });
  };

  const run = async (action: (r: StudyReport) => Promise<void>) => {
    setBusy(true);
    try {
      await action(await build());
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const print = () =>
    run(async (report) => {
      const letterhead = await loadDocumentHeaderHtml();
      const html = studyReportHtml(
        report,
        letterhead,
        t("rams.export.printedAt", { date: formatDateTime(new Date()) }),
        i18n.language,
      );
      await printHtml(html, { title: report.title });
    });

  const csv = () =>
    run(async (report) => {
      const base = scope === "study" ? t("rams.export.fileBase") : t("rams.export.noteFileBase");
      const path = await saveCsvWithDialog(
        `${base}-${study.id}-${new Date().toISOString().slice(0, 10)}.csv`,
        [t("rams.export.colSection"), t("rams.export.colItem"), t("rams.export.colValue")],
        studyReportCsvRows(report),
        t("rams.export.csv"),
      );
      pushAppToast({ title: t("rams.export.saved"), description: path, variant: "success" });
    });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" className="h-10 gap-1.5" disabled={busy}>
          <Download className="h-4 w-4" aria-hidden />
          {scope === "study" ? t("rams.export.button") : t("rams.export.noteButton")}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem className="min-h-11" onClick={() => void print()}>
          <Printer className="mr-2 h-4 w-4" aria-hidden />
          {t("rams.export.print")}
        </DropdownMenuItem>
        <DropdownMenuItem className="min-h-11" onClick={() => void csv()}>
          <FileSpreadsheet className="mr-2 h-4 w-4" aria-hidden />
          {t("rams.export.csv")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

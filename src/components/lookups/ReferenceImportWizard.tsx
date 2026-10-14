/**
 * ReferenceImportWizard.tsx
 *
 * Three-step CSV import wizard for reference values:
 *   Step 1 – Upload CSV file  →  createRefImportBatch + stageRefImportRows
 *   Step 2 – Map & Validate   →  validateRefImportBatch + getRefImportPreview
 *   Step 3 – Apply & Result   →  applyRefImportBatch
 *
 * Phase 2 – Sub-phase 03 – File 03 – Sprint S4 (GAP REF-05).
 */

import {
  AlertTriangle,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  FileSpreadsheet,
  Loader2,
  Upload,
  XCircle,
} from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { parseReferenceCsv } from "@/components/lookups/reference-csv";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone, type Tone, toneOf } from "@/design-system/tone";
import {
  applyRefImportBatch,
  createRefImportBatch,
  getRefImportPreview,
  stageRefImportRows,
  validateRefImportBatch,
} from "@/services/reference-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import type {
  ImportRowInput,
  RefImportApplyResult,
  RefImportBatchSummary,
  RefImportPreview,
  RefImportRow,
} from "@shared/ipc-types";

async function sha256Hex(text: string): Promise<string> {
  const data = new TextEncoder().encode(text);
  const hashBuffer = await crypto.subtle.digest("SHA-256", data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
}

// ── Component ─────────────────────────────────────────────────────────────────

interface ReferenceImportWizardProps {
  domainId: number;
  targetSetId: number;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onComplete?: () => void;
}

type Step = "upload" | "validate" | "result";

export function ReferenceImportWizard({
  domainId,
  targetSetId,
  open,
  onOpenChange,
  onComplete,
}: ReferenceImportWizardProps) {
  const { t } = useTranslation("reference");

  const [step, setStep] = useState<Step>("upload");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Step 1: Upload
  const [fileName, setFileName] = useState<string | null>(null);
  const [parsedRows, setParsedRows] = useState<ImportRowInput[]>([]);
  const [rawText, setRawText] = useState<string>("");

  // Step 2: Validate
  const [batch, setBatch] = useState<RefImportBatchSummary | null>(null);
  const [preview, setPreview] = useState<RefImportPreview | null>(null);
  const [includeWarnings, setIncludeWarnings] = useState(false);

  // Step 3: Result
  const [result, setResult] = useState<RefImportApplyResult | null>(null);

  // ── Reset ──────────────────────────────────────────────────────────────

  const reset = useCallback(() => {
    setStep("upload");
    setLoading(false);
    setError(null);
    setFileName(null);
    setParsedRows([]);
    setRawText("");
    setBatch(null);
    setPreview(null);
    setIncludeWarnings(false);
    setResult(null);
  }, []);

  const handleClose = useCallback(() => {
    onOpenChange(false);
    // Delay reset so the dialog closing animation completes
    setTimeout(reset, 200);
  }, [onOpenChange, reset]);

  // ── Step 1: File upload ────────────────────────────────────────────────

  const handleFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;

      setError(null);
      const text = await file.text();
      const rows = parseReferenceCsv(text);

      if (rows.length === 0) {
        setError(t("import.noRows"));
        return;
      }

      setFileName(file.name);
      setParsedRows(rows);
      setRawText(text);
    },
    [t],
  );

  const handleUploadAndStage = useCallback(async () => {
    if (!fileName || parsedRows.length === 0) return;
    setLoading(true);
    setError(null);
    try {
      const hash = await sha256Hex(rawText);
      const batchSummary = await createRefImportBatch(domainId, fileName, hash);
      const stagedBatch = await stageRefImportRows(batchSummary.id, parsedRows);
      setBatch(stagedBatch);
      setStep("validate");
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setLoading(false);
    }
  }, [fileName, parsedRows, rawText, domainId]);

  // ── Step 2: Validate & Preview ─────────────────────────────────────────

  const handleValidate = useCallback(async () => {
    if (!batch) return;
    setLoading(true);
    setError(null);
    try {
      const validated = await validateRefImportBatch(batch.id);
      setBatch(validated);
      const prev = await getRefImportPreview(batch.id);
      setPreview(prev);
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setLoading(false);
    }
  }, [batch]);

  // ── Step 3: Apply ──────────────────────────────────────────────────────

  const handleApply = useCallback(async () => {
    if (!batch) return;
    setLoading(true);
    setError(null);
    try {
      const res = await applyRefImportBatch(batch.id, {
        include_warnings: includeWarnings,
        target_set_id: targetSetId,
      });
      setResult(res);
      setStep("result");
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setLoading(false);
    }
  }, [batch, includeWarnings, targetSetId]);

  // ── Derived preview data ───────────────────────────────────────────────

  const warningRows = useMemo(
    () => preview?.rows.filter((r: RefImportRow) => r.validation_status === "warning") ?? [],
    [preview],
  );

  // Rows in error are never applied; the others are, warnings only when the user includes them.
  const applicableRows = batch ? batch.valid_rows + (includeWarnings ? batch.warning_rows : 0) : 0;
  const canApply = applicableRows > 0;

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? undefined : handleClose())}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <FileSpreadsheet className="h-5 w-5" />
            {t("import.title")}
          </DialogTitle>
          <DialogDescription>{t("import.description")}</DialogDescription>
        </DialogHeader>

        {/* Step indicator */}
        <div className="flex items-center gap-2 text-xs text-text-muted mb-2">
          <Badge variant={step === "upload" ? "default" : "secondary"} className="text-[10px]">
            1 – {t("import.stepUpload")}
          </Badge>
          <ChevronRight className="h-3 w-3" />
          <Badge variant={step === "validate" ? "default" : "secondary"} className="text-[10px]">
            2 – {t("import.stepValidate")}
          </Badge>
          <ChevronRight className="h-3 w-3" />
          <Badge variant={step === "result" ? "default" : "secondary"} className="text-[10px]">
            3 – {t("import.stepResult")}
          </Badge>
        </div>

        {/* Error banner */}
        {error && (
          <div
            role="alert"
            className={`flex items-start gap-2 rounded border p-3 text-xs ${mfTone.danger.panel} ${mfTone.danger.text}`}
          >
            <XCircle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
            <span>{error}</span>
          </div>
        )}

        {/* ── Step 1: Upload ──────────────────────────────────────────── */}
        {step === "upload" && (
          <div className="space-y-4">
            <div className="flex flex-col items-center justify-center border-2 border-dashed border-surface-border rounded-lg p-8 gap-3">
              <Upload className="h-8 w-8 text-text-muted/40" />
              <p className="text-sm text-text-muted">{t("import.dropzone")}</p>
              <label className="cursor-pointer">
                <input
                  type="file"
                  accept=".csv"
                  className="sr-only"
                  onChange={(e) => void handleFileChange(e)}
                />
                <span className="inline-flex items-center rounded-md border border-input bg-background px-3 py-1.5 text-sm font-medium shadow-sm hover:bg-accent">
                  {t("import.browse")}
                </span>
              </label>
            </div>

            {fileName && (
              <div className="flex items-center justify-between rounded bg-surface-1 px-3 py-2 text-sm">
                <span className="truncate">{fileName}</span>
                <Badge variant="secondary" className="text-[10px]">
                  {t("import.rowCount", { count: parsedRows.length })}
                </Badge>
              </div>
            )}
          </div>
        )}

        {/* ── Step 2: Validate ────────────────────────────────────────── */}
        {step === "validate" && (
          <div className="space-y-4">
            {/* Batch summary */}
            {batch && (
              <div className="grid grid-cols-4 gap-2 text-center">
                <SummaryCard label={t("import.totalRows")} value={batch.total_rows} />
                <SummaryCard
                  label={t("import.validRows")}
                  value={batch.valid_rows}
                  variant="success"
                />
                <SummaryCard
                  label={t("import.warningRows")}
                  value={batch.warning_rows}
                  variant="warning"
                />
                <SummaryCard
                  label={t("import.errorRows")}
                  value={batch.error_rows}
                  variant="error"
                />
              </div>
            )}

            {/* Validate button */}
            {!preview && (
              <Button
                onClick={() => void handleValidate()}
                disabled={loading}
                className="w-full gap-2"
              >
                {loading && <Loader2 className="h-4 w-4 animate-spin" />}
                {t("import.runValidation")}
              </Button>
            )}

            {/* Row preview table */}
            {preview && (
              <div className="max-h-64 overflow-auto border border-surface-border rounded">
                <table className="w-full text-xs">
                  <thead className="bg-surface-1 sticky top-0">
                    <tr>
                      <th className="px-2 py-1.5 text-left">#</th>
                      <th className="px-2 py-1.5 text-left">{t("import.colCode")}</th>
                      <th className="px-2 py-1.5 text-left">{t("import.colStatus")}</th>
                      <th className="px-2 py-1.5 text-left">{t("import.colAction")}</th>
                      <th className="px-2 py-1.5 text-left">{t("import.colMessages")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preview.rows.map((row: RefImportRow) => (
                      <tr key={row.id} className="border-t border-surface-border">
                        <td className="px-2 py-1">{row.row_no}</td>
                        <td className="px-2 py-1 font-mono">{row.normalized_code ?? "—"}</td>
                        <td className="px-2 py-1">
                          <RowStatusBadge status={row.validation_status} />
                        </td>
                        <td className="px-2 py-1">
                          {row.proposed_action
                            ? t(`import.action.${row.proposed_action}`, {
                                defaultValue: t("import.action.other"),
                              })
                            : "—"}
                        </td>
                        <td className="px-2 py-1 text-text-muted">
                          {row.messages.map((m) => (
                            <span key={`${row.id}-${m.category}-${m.severity}`} className="block">
                              {m.message}
                            </span>
                          ))}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {/* Include warnings checkbox */}
            {preview && warningRows.length > 0 && (
              <div className="flex items-center gap-2 text-sm">
                <Checkbox
                  id="include-warnings"
                  checked={includeWarnings}
                  onCheckedChange={(v) => setIncludeWarnings(v === true)}
                />
                <label htmlFor="include-warnings">
                  {t("import.includeWarnings", { count: warningRows.length })}
                </label>
              </div>
            )}
          </div>
        )}

        {/* ── Step 3: Result ──────────────────────────────────────────── */}
        {step === "result" && result && (
          <div className="space-y-4 text-center py-4">
            <CheckCircle2 className="h-10 w-10 mx-auto text-status-success" />
            <p className="text-sm font-medium">{t("import.applied")}</p>
            <div className="grid grid-cols-4 gap-2">
              <SummaryCard label={t("import.created")} value={result.created} variant="success" />
              <SummaryCard label={t("import.updated")} value={result.updated} />
              <SummaryCard label={t("import.skipped")} value={result.skipped} />
              <SummaryCard label={t("import.errored")} value={result.errored} variant="error" />
            </div>
          </div>
        )}

        {/* ── Footer ──────────────────────────────────────────────────── */}
        <DialogFooter>
          {step === "upload" && (
            <Button
              onClick={() => void handleUploadAndStage()}
              disabled={!fileName || parsedRows.length === 0 || loading}
              className="gap-2"
            >
              {loading && <Loader2 className="h-4 w-4 animate-spin" />}
              {t("import.next")}
              <ChevronRight className="h-4 w-4" />
            </Button>
          )}

          {step === "validate" && (
            <>
              <Button variant="outline" onClick={() => setStep("upload")} className="gap-1.5">
                <ChevronLeft className="h-4 w-4" />
                {t("import.back")}
              </Button>
              <Button
                onClick={() => void handleApply()}
                disabled={!canApply || loading || !preview}
                className="gap-2"
              >
                {loading && <Loader2 className="h-4 w-4 animate-spin" />}
                {t("import.apply")}
              </Button>
            </>
          )}

          {step === "result" && (
            <Button
              onClick={() => {
                handleClose();
                onComplete?.();
              }}
            >
              {t("import.done")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Small helpers ─────────────────────────────────────────────────────────────

function SummaryCard({
  label,
  value,
  variant,
}: {
  label: string;
  value: number;
  variant?: "success" | "warning" | "error";
}) {
  const colors = {
    success: mfTone.success.text,
    warning: mfTone.warning.text,
    error: mfTone.danger.text,
  };
  return (
    <div className="rounded border border-surface-border p-2">
      <p className="text-xs text-text-muted">{label}</p>
      <p className={`text-lg font-semibold ${variant ? colors[variant] : "text-text-primary"}`}>
        {value}
      </p>
    </div>
  );
}

const ROW_STATUS_TONE: Record<string, Tone> = {
  valid: "success",
  warning: "warning",
  error: "danger",
};

function RowStatusBadge({ status }: { status: string }) {
  const { t } = useTranslation("reference");
  const tone = toneOf(ROW_STATUS_TONE, status);
  const icon = status === "valid" ? CheckCircle2 : status === "error" ? XCircle : AlertTriangle;
  return (
    <StatusChip
      label={t(`import.rowStatus.${status}`, { defaultValue: t("import.rowStatus.other") })}
      tone={tone}
      icon={icon}
    />
  );
}

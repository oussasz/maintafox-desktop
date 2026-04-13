/**
 * AssetImportPage.tsx
 *
 * SP02-F04-S3 — Guided import workflow for bulk asset onboarding.
 *
 * Steps: upload → validate → preview/inspect → apply → outcome summary.
 */

import {
  AlertTriangle,
  CheckCircle2,
  FileUp,
  Loader2,
  Play,
  RotateCcw,
  Search,
  Shield,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { useAssetImportStore, type ImportStep } from "@/stores/asset-import-store";

// ── Step indicator ────────────────────────────────────────────────────────────

const STEPS: { key: ImportStep; labelKey: string; icon: typeof FileUp }[] = [
  { key: "upload", labelKey: "import.steps.upload", icon: FileUp },
  { key: "validate", labelKey: "import.steps.validate", icon: Shield },
  { key: "preview", labelKey: "import.steps.preview", icon: Search },
  { key: "apply", labelKey: "import.steps.apply", icon: Play },
  { key: "done", labelKey: "import.steps.done", icon: CheckCircle2 },
];

function StepIndicator({ current }: { current: ImportStep }) {
  const { t } = useTranslation("equipment");
  const currentIdx = STEPS.findIndex((s) => s.key === current);

  return (
    <nav aria-label="Import steps" className="flex items-center gap-2">
      {STEPS.map((step, idx) => {
        const Icon = step.icon;
        const isActive = idx === currentIdx;
        const isDone = idx < currentIdx;

        return (
          <div key={step.key} className="flex items-center gap-2">
            {idx > 0 && (
              <div className={cn("h-px w-6", isDone ? "bg-status-success" : "bg-surface-border")} />
            )}
            <div
              className={cn(
                "flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium",
                isActive && "bg-primary/10 text-primary",
                isDone && "text-status-success",
                !isActive && !isDone && "text-text-muted",
              )}
            >
              <Icon className="h-3.5 w-3.5" />
              <span>{t(step.labelKey as never)}</span>
            </div>
          </div>
        );
      })}
    </nav>
  );
}

// ── Validation status badge ───────────────────────────────────────────────────

function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation("equipment");

  switch (status) {
    case "valid":
      return (
        <Badge variant="outline" className="border-status-success text-status-success">
          <CheckCircle2 className="mr-1 h-3 w-3" />
          {t("import.status.valid")}
        </Badge>
      );
    case "warning":
      return (
        <Badge variant="outline" className="border-status-warning text-status-warning">
          <AlertTriangle className="mr-1 h-3 w-3" />
          {t("import.status.warning")}
        </Badge>
      );
    case "error":
      return (
        <Badge variant="outline" className="border-status-danger text-status-danger">
          <XCircle className="mr-1 h-3 w-3" />
          {t("import.status.error")}
        </Badge>
      );
    default:
      return <Badge variant="outline">{status}</Badge>;
  }
}

// ── Upload step ───────────────────────────────────────────────────────────────

function UploadStep() {
  const { t } = useTranslation("equipment");
  const loading = useAssetImportStore((s) => s.loading);
  const error = useAssetImportStore((s) => s.error);
  const uploadAndCreateBatch = useAssetImportStore((s) => s.uploadAndCreateBatch);
  const inputRef = useRef<HTMLInputElement>(null);

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (file) void uploadAndCreateBatch(file);
    },
    [uploadAndCreateBatch],
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <FileUp className="h-5 w-5" />
          {t("import.upload.title")}
        </CardTitle>
        <CardDescription>{t("import.upload.description")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col items-center gap-4 rounded-lg border-2 border-dashed border-surface-border p-8">
          <FileUp className="h-10 w-10 text-text-muted" />
          <p className="text-sm text-text-muted">{t("import.upload.hint")}</p>
          <input
            ref={inputRef}
            type="file"
            accept=".csv"
            className="hidden"
            onChange={handleFileChange}
            data-testid="file-input"
          />
          <Button onClick={() => inputRef.current?.click()} disabled={loading}>
            {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t("import.upload.selectFile")}
          </Button>
        </div>
        {error && <p className="mt-3 text-sm text-status-danger">{error}</p>}
      </CardContent>
    </Card>
  );
}

// ── Validate step ─────────────────────────────────────────────────────────────

function ValidateStep() {
  const { t } = useTranslation("equipment");
  const batch = useAssetImportStore((s) => s.batch);
  const loading = useAssetImportStore((s) => s.loading);
  const error = useAssetImportStore((s) => s.error);
  const validateBatch = useAssetImportStore((s) => s.validateBatch);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Shield className="h-5 w-5" />
          {t("import.validate.title")}
        </CardTitle>
        <CardDescription>
          {t("import.validate.description", {
            filename: batch?.source_filename,
            rows: batch?.total_rows ?? 0,
          })}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        {batch && (
          <div className="flex gap-4 text-sm text-text-muted">
            <span>
              {t("import.validate.file")}: <strong>{batch.source_filename}</strong>
            </span>
            <span>
              {t("import.validate.rows")}: <strong>{batch.total_rows}</strong>
            </span>
          </div>
        )}
        {error && <p className="text-sm text-status-danger">{error}</p>}
      </CardContent>
      <CardFooter>
        <Button onClick={() => void validateBatch()} disabled={loading}>
          {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {t("import.validate.run")}
        </Button>
      </CardFooter>
    </Card>
  );
}

// ── Preview step (conflict table) ─────────────────────────────────────────────

function PreviewStep() {
  const { t } = useTranslation("equipment");
  const batch = useAssetImportStore((s) => s.batch);
  const previewRows = useAssetImportStore((s) => s.previewRows);
  const includeWarnings = useAssetImportStore((s) => s.includeWarnings);
  const setIncludeWarnings = useAssetImportStore((s) => s.setIncludeWarnings);
  const applyBatch = useAssetImportStore((s) => s.applyBatch);
  const saving = useAssetImportStore((s) => s.saving);
  const error = useAssetImportStore((s) => s.error);

  const canApply = batch?.status === "validated";

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Search className="h-5 w-5" />
          {t("import.preview.title")}
        </CardTitle>
        <CardDescription>{t("import.preview.description")}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Summary badges */}
        {batch && (
          <div className="flex flex-wrap gap-3" data-testid="summary-badges">
            <Badge variant="outline">
              {t("import.preview.total")}: {batch.total_rows}
            </Badge>
            <Badge variant="outline" className="border-status-success text-status-success">
              {t("import.status.valid")}: {batch.valid_rows}
            </Badge>
            <Badge variant="outline" className="border-status-warning text-status-warning">
              {t("import.status.warning")}: {batch.warning_rows}
            </Badge>
            <Badge variant="outline" className="border-status-danger text-status-danger">
              {t("import.status.error")}: {batch.error_rows}
            </Badge>
          </div>
        )}

        {/* Conflict / preview table */}
        <div className="max-h-[400px] overflow-auto rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-12">#</TableHead>
                <TableHead>{t("import.preview.columns.code")}</TableHead>
                <TableHead>{t("import.preview.columns.externalKey")}</TableHead>
                <TableHead>{t("import.preview.columns.status")}</TableHead>
                <TableHead>{t("import.preview.columns.action")}</TableHead>
                <TableHead>{t("import.preview.columns.messages")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {previewRows.map((row) => (
                <TableRow
                  key={row.id}
                  className={cn(
                    row.validation_status === "error" && "bg-status-danger/5",
                    row.validation_status === "warning" && "bg-status-warning/5",
                  )}
                  data-testid={`preview-row-${row.row_no}`}
                >
                  <TableCell className="text-text-muted">{row.row_no}</TableCell>
                  <TableCell className="font-mono text-xs">
                    {row.normalized_asset_code ?? "—"}
                  </TableCell>
                  <TableCell className="font-mono text-xs">
                    {row.normalized_external_key ?? "—"}
                  </TableCell>
                  <TableCell>
                    <StatusBadge status={row.validation_status} />
                  </TableCell>
                  <TableCell>
                    {row.proposed_action ? (
                      <Badge variant="secondary">{row.proposed_action}</Badge>
                    ) : (
                      "—"
                    )}
                  </TableCell>
                  <TableCell>
                    {row.validation_messages.length > 0 ? (
                      <ul className="space-y-0.5">
                        {row.validation_messages.map((msg, idx) => (
                          <li
                            key={idx}
                            className={cn(
                              "text-xs",
                              msg.severity === "error" && "text-status-danger",
                              msg.severity === "warning" && "text-status-warning",
                            )}
                            data-testid="conflict-message"
                          >
                            <span className="font-medium">[{msg.category}]</span> {msg.message}
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <span className="text-xs text-text-muted">—</span>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>

        {/* Policy toggles */}
        <div className="flex items-center gap-3">
          <input
            type="checkbox"
            id="include-warnings"
            checked={includeWarnings}
            onChange={(e) => setIncludeWarnings(e.target.checked)}
            className="h-4 w-4 rounded border"
          />
          <Label htmlFor="include-warnings">{t("import.preview.includeWarnings")}</Label>
        </div>

        {error && <p className="text-sm text-status-danger">{error}</p>}
      </CardContent>
      <CardFooter className="flex gap-3">
        <Button
          onClick={() => void applyBatch()}
          disabled={!canApply || saving}
          data-testid="apply-button"
        >
          {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          <Play className="mr-2 h-4 w-4" />
          {t("import.apply.run")}
        </Button>
      </CardFooter>
    </Card>
  );
}

// ── Done step (outcome summary) ───────────────────────────────────────────────

function DoneStep() {
  const { t } = useTranslation("equipment");
  const applyResult = useAssetImportStore((s) => s.applyResult);
  const resetFlow = useAssetImportStore((s) => s.resetFlow);

  if (!applyResult) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-status-success">
          <CheckCircle2 className="h-5 w-5" />
          {t("import.done.title")}
        </CardTitle>
        <CardDescription>{t("import.done.description")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4" data-testid="outcome-summary">
          <SummaryCard
            label={t("import.done.created")}
            value={applyResult.created}
            className="text-status-success"
          />
          <SummaryCard
            label={t("import.done.updated")}
            value={applyResult.updated}
            className="text-primary"
          />
          <SummaryCard
            label={t("import.done.skipped")}
            value={applyResult.skipped}
            className="text-text-muted"
          />
          <SummaryCard
            label={t("import.done.errored")}
            value={applyResult.errored}
            className="text-status-danger"
          />
        </div>
      </CardContent>
      <CardFooter>
        <Button variant="outline" onClick={resetFlow}>
          <RotateCcw className="mr-2 h-4 w-4" />
          {t("import.done.newImport")}
        </Button>
      </CardFooter>
    </Card>
  );
}

function SummaryCard({
  label,
  value,
  className,
}: {
  label: string;
  value: number;
  className?: string;
}) {
  return (
    <div className="flex flex-col items-center rounded-lg border p-3">
      <span className={cn("text-2xl font-bold", className)}>{value}</span>
      <span className="text-xs text-text-muted">{label}</span>
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────────────────

export function AssetImportPage() {
  const { t } = useTranslation("equipment");
  const step = useAssetImportStore((s) => s.step);
  const resetFlow = useAssetImportStore((s) => s.resetFlow);

  useEffect(() => {
    return () => resetFlow();
  }, [resetFlow]);

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold text-text-primary">{t("import.page.title")}</h1>
        <StepIndicator current={step} />
      </div>

      {/* Active step */}
      <div className="flex-1">
        {step === "upload" && <UploadStep />}
        {step === "validate" && <ValidateStep />}
        {(step === "preview" || step === "apply") && <PreviewStep />}
        {step === "done" && <DoneStep />}
      </div>
    </div>
  );
}

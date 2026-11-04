/**
 * DataImportPage.tsx
 *
 * "Imports de données": one wizard (choose the data → template / export → file → preview with the
 * reason for every refused line → apply) for the plant data that has no dedicated importer.
 * Each data type comes from the backend catalogue, so a new type needs no new screen.
 */

import {
  AlertTriangle,
  CheckCircle2,
  Download,
  FileUp,
  Loader2,
  Play,
  RotateCcw,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { StatusChip } from "@/components/ui/status-chip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { saveCsvWithDialog } from "@/export/csv";
import { isExportCancelled } from "@/export/types";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  applyBulkImportBatch,
  createBulkImportBatch,
  exportBulkImportData,
  getBulkImportPreview,
  getBulkImportTemplate,
  listBulkImportEntities,
  type BulkImportApplyResult,
  type BulkImportEntity,
  type BulkImportPreview,
} from "@/services/bulk-import-service";
import { pushAppToast } from "@/store/app-toast-store";

async function sha256Hex(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function sourceKind(name: string): "csv" | "xlsx" | null {
  const lower = name.toLowerCase();
  if (lower.endsWith(".xlsx")) return "xlsx";
  if (lower.endsWith(".csv") || lower.endsWith(".txt")) return "csv";
  return null;
}

const GUIDE_STEPS = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7"] as const;

export function DataImportPage() {
  const { t } = useTranslation("imports");
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const fileInput = useRef<HTMLInputElement>(null);

  const [entities, setEntities] = useState<BulkImportEntity[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<BulkImportPreview | null>(null);
  const [result, setResult] = useState<BulkImportApplyResult | null>(null);
  const [confirmApply, setConfirmApply] = useState(false);

  useEffect(() => {
    listBulkImportEntities()
      .then((list) => {
        setEntities(list);
        setSelected((cur) => cur ?? list[0]?.code ?? null);
      })
      .catch((err: unknown) => setLoadError(appErrorReason(err)));
  }, []);

  const entity = entities.find((e) => e.code === selected) ?? null;

  const reset = useCallback(() => {
    setPreview(null);
    setResult(null);
    setError(null);
  }, []);

  const saveTable = useCallback(
    async (kind: "template" | "export") => {
      if (!entity) return;
      try {
        const table =
          kind === "template"
            ? await getBulkImportTemplate(entity.code)
            : await exportBulkImportData(entity.code);
        const file = `${kind === "template" ? "modele" : "export"}-import-${entity.code}.csv`;
        const path = await saveCsvWithDialog(file, table.header, table.rows);
        pushAppToast({ title: t("file.saved"), description: path, variant: "success" });
      } catch (err) {
        if (isExportCancelled(err)) return;
        pushAppToast({
          title: t("file.saveFailed"),
          description: appErrorReason(err),
          variant: "destructive",
        });
      }
    },
    [entity, t],
  );

  const onFile = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      e.target.value = "";
      if (!file || !entity) return;
      const kind = sourceKind(file.name);
      if (!kind) {
        setError(t("file.badType"));
        return;
      }
      setBusy(true);
      reset();
      try {
        const buffer = await file.arrayBuffer();
        const batch = await createBulkImportBatch({
          entity: entity.code,
          filename: file.name,
          source_sha256: await sha256Hex(buffer),
          source_kind: kind,
          file_content: Array.from(new Uint8Array(buffer)),
        });
        setPreview(await getBulkImportPreview(batch.id));
      } catch (err) {
        setError(appErrorReason(err));
      } finally {
        setBusy(false);
      }
    },
    [entity, reset, t],
  );

  const apply = useCallback(async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      const res = await withStepUp(() => applyBulkImportBatch(preview.batch.id));
      setResult(res);
      setPreview(await getBulkImportPreview(preview.batch.id));
    } catch (err) {
      if (!isStepUpCancelled(err)) setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  }, [preview, withStepUp]);

  const importable = preview
    ? preview.rows.filter((r) => r.status !== "error" && r.action !== "skip").length
    : 0;
  const applied = preview?.batch.status === "applied";

  return (
    <ModulePageShell icon={FileUp} title={t("title")} description={t("description")}>
      <div className="space-y-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t("guide.title")}</CardTitle>
            <CardDescription>{t("guide.intro")}</CardDescription>
          </CardHeader>
          <CardContent>
            <ol className="list-decimal space-y-1 pl-5 text-sm">
              {GUIDE_STEPS.map((s) => (
                <li key={s}>{t(`guide.${s}`)}</li>
              ))}
            </ol>
            <p className="mt-3 text-xs text-text-muted">{t("guide.rules")}</p>
          </CardContent>
        </Card>

        {loadError && (
          <p role="alert" className="text-sm text-tone-danger-fg">
            {loadError}
          </p>
        )}

        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t("choose.title")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex flex-wrap gap-2">
              {entities.map((e) => (
                <Button
                  key={e.code}
                  variant={e.code === selected ? "default" : "outline"}
                  size="sm"
                  disabled={busy}
                  onClick={() => {
                    setSelected(e.code);
                    reset();
                  }}
                >
                  {e.label}
                </Button>
              ))}
            </div>

            {entity && (
              <div className="space-y-3">
                <p className="text-sm text-text-muted">
                  {t("choose.needs")} {entity.needs}
                </p>
                <div className="overflow-auto rounded-md border">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t("columns.name")}</TableHead>
                        <TableHead>{t("columns.required")}</TableHead>
                        <TableHead>{t("columns.hint")}</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {entity.columns.map((c) => (
                        <TableRow key={c.key}>
                          <TableCell className="font-mono text-xs">{c.header}</TableCell>
                          <TableCell>{c.required ? t("columns.yes") : t("columns.no")}</TableCell>
                          <TableCell className="text-sm text-text-muted">{c.hint}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => void saveTable("template")}
                  >
                    <Download className="mr-2 h-4 w-4" />
                    {t("file.template")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => void saveTable("export")}
                  >
                    <Download className="mr-2 h-4 w-4" />
                    {t("file.export")}
                  </Button>
                  <input
                    ref={fileInput}
                    type="file"
                    accept=".csv,.txt,.xlsx"
                    className="hidden"
                    onChange={(e) => void onFile(e)}
                    data-testid="bulk-import-file"
                  />
                  <Button size="sm" disabled={busy} onClick={() => fileInput.current?.click()}>
                    {busy ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <FileUp className="mr-2 h-4 w-4" />
                    )}
                    {t("file.choose")}
                  </Button>
                </div>
              </div>
            )}
            {error && (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {error}
              </p>
            )}
          </CardContent>
        </Card>

        {preview && (
          <Card>
            <CardHeader>
              <CardTitle className="text-base">
                {t("preview.title", { file: preview.batch.source_filename })}
              </CardTitle>
              <CardDescription>
                {applied ? t("preview.appliedHint") : t("preview.hint")}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex flex-wrap gap-2">
                <StatusChip
                  size="md"
                  tone="neutral"
                  label={t("preview.total", { n: preview.batch.total_rows })}
                />
                <StatusChip
                  size="md"
                  tone="success"
                  label={t("preview.ok", { n: preview.batch.valid_rows })}
                />
                <StatusChip
                  size="md"
                  tone="warning"
                  label={t("preview.warnings", { n: preview.batch.warning_rows })}
                />
                <StatusChip
                  size="md"
                  tone="danger"
                  label={t("preview.errors", { n: preview.batch.error_rows })}
                />
              </div>
              {preview.batch.ignored_columns.length > 0 && (
                <p className="text-xs text-text-muted">
                  {t("preview.ignored", { columns: preview.batch.ignored_columns.join(", ") })}
                </p>
              )}
              <div className="max-h-[420px] overflow-auto rounded-md border">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="w-12">#</TableHead>
                      <TableHead>{t("preview.data")}</TableHead>
                      <TableHead>{t("preview.status")}</TableHead>
                      <TableHead>{t("preview.action")}</TableHead>
                      <TableHead>{t("preview.messages")}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {preview.rows.map((row) => (
                      <TableRow
                        key={row.row_no}
                        className={cn(
                          row.status === "error" && "bg-tone-danger-bg/40",
                          row.status === "warning" && "bg-tone-warning-bg/40",
                        )}
                      >
                        <TableCell className="text-text-muted">{row.row_no}</TableCell>
                        <TableCell className="font-mono text-xs">
                          {(entities.find((e) => e.code === preview.batch.entity)?.columns ?? [])
                            .map((c) => row.raw[c.key])
                            .filter(Boolean)
                            .join(" · ")}
                        </TableCell>
                        <TableCell>
                          <StatusChip
                            icon={
                              row.status === "ok"
                                ? CheckCircle2
                                : row.status === "warning"
                                  ? AlertTriangle
                                  : XCircle
                            }
                            tone={
                              row.status === "ok"
                                ? "success"
                                : row.status === "warning"
                                  ? "warning"
                                  : "danger"
                            }
                            label={t(`preview.s.${row.status}`)}
                          />
                        </TableCell>
                        <TableCell>
                          {row.apply_status ? (
                            <span className="text-xs">
                              {t(`preview.applied.${row.apply_status}`)}
                            </span>
                          ) : (
                            <span className="text-xs">{t(`preview.a.${row.action}`)}</span>
                          )}
                        </TableCell>
                        <TableCell>
                          <ul className="space-y-0.5">
                            {row.messages.map((m, i) => (
                              <li
                                key={i}
                                className={cn(
                                  "text-xs",
                                  m.severity === "error"
                                    ? "text-tone-danger-fg"
                                    : "text-tone-warning-fg",
                                )}
                              >
                                {m.message}
                              </li>
                            ))}
                            {row.apply_status === "failed" && row.apply_message && (
                              <li className="text-xs text-tone-danger-fg">{row.apply_message}</li>
                            )}
                          </ul>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>

              {result && (
                <p className="text-sm font-medium" data-testid="bulk-import-result">
                  {t("result.summary", {
                    created: result.created,
                    updated: result.updated,
                    skipped: result.skipped,
                    failed: result.failed,
                  })}
                </p>
              )}

              <div className="flex flex-wrap gap-2">
                {!applied && (
                  <Button
                    disabled={busy || importable === 0}
                    onClick={() => setConfirmApply(true)}
                    data-testid="bulk-import-apply"
                  >
                    {busy ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <Play className="mr-2 h-4 w-4" />
                    )}
                    {t("apply.run", { n: importable })}
                  </Button>
                )}
                <Button variant="outline" disabled={busy} onClick={reset}>
                  <RotateCcw className="mr-2 h-4 w-4" />
                  {applied ? t("apply.another") : t("apply.cancel")}
                </Button>
              </div>
              {!applied && importable === 0 && (
                <p className="text-sm text-tone-warning-fg">{t("apply.nothing")}</p>
              )}
            </CardContent>
          </Card>
        )}
      </div>

      <ConfirmDialog
        open={confirmApply}
        onOpenChange={setConfirmApply}
        title={t("apply.confirmTitle")}
        description={t("apply.confirmBody", {
          n: importable,
          errors: preview?.batch.error_rows ?? 0,
        })}
        confirmLabel={t("apply.confirmLabel")}
        onConfirm={() => {
          setConfirmApply(false);
          void apply();
        }}
      />
      {StepUpDialogElement}
    </ModulePageShell>
  );
}

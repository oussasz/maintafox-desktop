import { FileUp, Loader2, ShieldCheck, Upload } from "lucide-react";
import { type ChangeEvent, useCallback, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  applyPersonnelImportBatch,
  createPersonnelImportBatch,
  getPersonnelImportPreview,
} from "@/services/personnel-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  PersonnelImportApplyResult,
  PersonnelImportBatchSummary,
  PersonnelImportPreview,
} from "@shared/ipc-types";

type ImportMode = "create_and_update" | "create_only";

async function sha256(file: File): Promise<string> {
  const buffer = await file.arrayBuffer();
  const hashBuffer = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function sourceKindFromName(fileName: string): "csv" | "xlsx" {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".xlsx")) return "xlsx";
  return "csv";
}

export function PersonnelImportWizard() {
  const { t } = useTranslation("personnel");
  const fileRef = useRef<HTMLInputElement | null>(null);
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<ImportMode>("create_and_update");
  const [batch, setBatch] = useState<PersonnelImportBatchSummary | null>(null);
  const [preview, setPreview] = useState<PersonnelImportPreview | null>(null);
  const [applyResult, setApplyResult] = useState<PersonnelImportApplyResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reset = useCallback(() => {
    setBatch(null);
    setPreview(null);
    setApplyResult(null);
    setError(null);
    setLoading(false);
    setApplying(false);
  }, []);

  const onOpenChange = useCallback(
    (next: boolean) => {
      setOpen(next);
      if (!next) reset();
    },
    [reset],
  );

  const handleSelectFile = useCallback(
    async (file: File) => {
      setLoading(true);
      setError(null);
      try {
        const source_kind = sourceKindFromName(file.name);
        const file_content = Array.from(new Uint8Array(await file.arrayBuffer()));
        const source_sha256 = await sha256(file);
        const created = await createPersonnelImportBatch({
          filename: file.name,
          source_kind,
          source_sha256,
          mode,
          file_content,
        });
        setBatch(created);
        const loadedPreview = await getPersonnelImportPreview(created.id);
        setPreview(loadedPreview);
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setLoading(false);
      }
    },
    [mode],
  );

  const onFileChange = useCallback(
    async (event: ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
      if (!file) return;
      await handleSelectFile(file);
    },
    [handleSelectFile],
  );

  const applyImport = useCallback(async () => {
    if (!batch) return;
    setApplying(true);
    setError(null);
    try {
      const result = await applyPersonnelImportBatch(batch.id);
      setApplyResult(result);
      const refreshed = await getPersonnelImportPreview(batch.id);
      setPreview(refreshed);
      setBatch(result.batch);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setApplying(false);
    }
  }, [batch]);

  const canApply = useMemo(() => batch?.status === "validated", [batch?.status]);

  return (
    <PermissionGate permission="per.manage">
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogTrigger asChild>
          <Button variant="outline" size="sm" className="gap-1.5">
            <FileUp className="h-3.5 w-3.5" />
            {t("import.action.openWizard")}
          </Button>
        </DialogTrigger>
        <DialogContent className="max-w-5xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Upload className="h-4 w-4" />
              {t("import.title")}
            </DialogTitle>
            <DialogDescription>{t("import.description")}</DialogDescription>
          </DialogHeader>

          <div className="space-y-3">
            <div className="space-y-2">
              <Label>{t("import.mode.label")}</Label>
              <Select value={mode} onValueChange={(v) => setMode(v as ImportMode)}>
                <SelectTrigger className="max-w-[280px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="create_and_update">{t("import.mode.createAndUpdate")}</SelectItem>
                  <SelectItem value="create_only">{t("import.mode.createOnly")}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="flex items-center gap-2">
              <Input
                ref={fileRef}
                type="file"
                className="max-w-sm"
                accept=".csv,.xlsx"
                onChange={(e) => void onFileChange(e)}
              />
              {loading ? <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" /> : null}
            </div>

            {batch ? (
              <div className="flex flex-wrap items-center gap-2">
                <Badge variant="outline">{t("import.badges.total", { count: batch.total_rows })}</Badge>
                <Badge variant="outline">{t("import.badges.valid", { count: batch.valid_rows })}</Badge>
                <Badge variant="outline">{t("import.badges.warning", { count: batch.warning_rows })}</Badge>
                <Badge variant="outline">{t("import.badges.error", { count: batch.error_rows })}</Badge>
              </div>
            ) : null}

            {error ? <div className="text-sm text-destructive">{error}</div> : null}

            {preview ? (
              <div className="h-[320px] overflow-auto rounded-md border">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>#</TableHead>
                      <TableHead>{t("import.table.employeeCode")}</TableHead>
                      <TableHead>{t("import.table.externalId")}</TableHead>
                      <TableHead>{t("import.table.status")}</TableHead>
                      <TableHead>{t("import.table.action")}</TableHead>
                      <TableHead>{t("import.table.messages")}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {preview.rows.map((row) => (
                      <TableRow key={row.id}>
                        <TableCell>{row.row_no}</TableCell>
                        <TableCell className="font-mono text-xs">{row.employee_code ?? "-"}</TableCell>
                        <TableCell className="font-mono text-xs">{row.hr_external_id ?? "-"}</TableCell>
                        <TableCell>{row.validation_status}</TableCell>
                        <TableCell>{row.proposed_action ?? "-"}</TableCell>
                        <TableCell className="max-w-[350px]">
                          {row.messages.map((msg, idx) => (
                            <div key={`${row.id}-${idx}`} className="text-xs text-muted-foreground">
                              [{msg.category}] {msg.message}
                            </div>
                          ))}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            ) : null}

            {applyResult ? (
              <div className="rounded-md border p-3 text-sm">
                <div className="mb-1 flex items-center gap-1 font-medium">
                  <ShieldCheck className="h-4 w-4" />
                  {t("import.applyResult.title")}
                </div>
                <div className="text-muted-foreground">
                  {t("import.applyResult.values", {
                    created: applyResult.created,
                    updated: applyResult.updated,
                    skipped: applyResult.skipped,
                    protected: applyResult.protected_ignored,
                  })}
                </div>
              </div>
            ) : null}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>
              {t("import.action.close")}
            </Button>
            <Button onClick={() => void applyImport()} disabled={!canApply || applying}>
              {applying ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {t("import.action.apply")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </PermissionGate>
  );
}

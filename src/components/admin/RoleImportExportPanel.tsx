import { AlertTriangle, CheckCircle, Download, Upload, XCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { useToast } from "@/hooks/use-toast";
import {
  exportRoleModel,
  importRoleModel,
  listRoles,
  validateRolePermissions,
} from "@/services/rbac-service";
import type {
  ImportResult,
  RoleImportEntry,
  RoleValidationResult,
  RoleWithPermissions,
} from "@shared/ipc-types";

// ── Preview row for import ───────────────────────────────────────────────

interface ImportPreviewRow {
  entry: RoleImportEntry;
  validation: RoleValidationResult | null;
  validating: boolean;
}

export function RoleImportExportPanel() {
  const { t } = useTranslation("admin");
  const { can } = usePermissions();
  const { toast } = useToast();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [roles, setRoles] = useState<RoleWithPermissions[]>([]);
  const [loading, setLoading] = useState(true);

  // Export state
  const [selectedExport, setSelectedExport] = useState<Set<number>>(new Set());

  // Import state
  const [importPreview, setImportPreview] = useState<ImportPreviewRow[] | null>(null);
  const [importResult, setImportResult] = useState<ImportResult | null>(null);
  const [importing, setImporting] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const fetchRoles = useCallback(async () => {
    try {
      const data = await listRoles();
      setRoles(data);
    } catch {
      toast({
        title: t("importExport.errors.loadFailed", "Failed to load roles"),
        variant: "destructive",
      });
    } finally {
      setLoading(false);
    }
  }, [t, toast]);

  useEffect(() => {
    void fetchRoles();
  }, [fetchRoles]);

  // ── Export ──────────────────────────────────────────────────────────────

  const toggleExportRole = useCallback((roleId: number) => {
    setSelectedExport((prev) => {
      const next = new Set(prev);
      if (next.has(roleId)) {
        next.delete(roleId);
      } else {
        next.add(roleId);
      }
      return next;
    });
  }, []);

  const handleExport = useCallback(async () => {
    if (selectedExport.size === 0) return;
    try {
      const payload = await exportRoleModel([...selectedExport]);
      const json = JSON.stringify(payload, null, 2);
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `role-export-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast({ title: t("importExport.export.success", "Export completed"), variant: "success" });
    } catch {
      toast({
        title: t("importExport.errors.exportFailed", "Failed to export"),
        variant: "destructive",
      });
    }
  }, [selectedExport, t, toast]);

  // ── Import ─────────────────────────────────────────────────────────────

  const handleFileSelect = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;

      try {
        const text = await file.text();
        const parsed = JSON.parse(text) as { roles?: RoleImportEntry[] };
        const entries = Array.isArray(parsed.roles) ? parsed.roles : [];

        if (entries.length === 0) {
          toast({
            title: t("importExport.import.parseError", "Failed to parse JSON file"),
            variant: "destructive",
          });
          return;
        }

        // Build preview rows with validation
        const preview: ImportPreviewRow[] = entries.map((entry) => ({
          entry,
          validation: null,
          validating: true,
        }));
        setImportPreview(preview);
        setImportResult(null);

        // Validate each role's permissions
        for (let i = 0; i < entries.length; i++) {
          const entry = entries[i];
          if (!entry) continue;
          try {
            const result = await validateRolePermissions(entry.permissions);
            setImportPreview(
              (prev) =>
                prev?.map((row, idx) =>
                  idx === i ? { ...row, validation: result, validating: false } : row,
                ) ?? null,
            );
          } catch {
            setImportPreview(
              (prev) =>
                prev?.map((row, idx) => (idx === i ? { ...row, validating: false } : row)) ?? null,
            );
          }
        }
      } catch {
        toast({
          title: t("importExport.import.parseError", "Failed to parse JSON file"),
          variant: "destructive",
        });
      }
      // Reset file input to allow re-selecting the same file
      if (fileInputRef.current) fileInputRef.current.value = "";
    },
    [t, toast],
  );

  const handleImport = useCallback(async () => {
    if (!importPreview) return;
    setImporting(true);
    try {
      const payload = {
        roles: importPreview.map((r) => r.entry),
      };
      const result = await withStepUp(() => importRoleModel(payload));
      setImportResult(result);
      toast({
        title: t("importExport.import.success", "Import complete: {{count}} role(s) imported", {
          count: result.imported_count,
        }),
        variant: "success",
      });
      void fetchRoles();
    } catch {
      toast({
        title: t("importExport.errors.importFailed", "Failed to import"),
        variant: "destructive",
      });
    } finally {
      setImporting(false);
    }
  }, [importPreview, fetchRoles, t, toast, withStepUp]);

  // ── Preview status helpers ──────────────────────────────────────────────

  const getPreviewStatus = (row: ImportPreviewRow) => {
    if (row.validating) return "validating";
    if (!row.validation) return "unknown";
    if (
      row.validation.missing_hard_deps.length > 0 ||
      row.validation.unknown_permissions.length > 0
    )
      return "blocked";
    if (row.validation.warn_deps.length > 0) return "warnings";
    return "valid";
  };

  const hasBlockedRoles = useMemo(
    () => importPreview?.some((r) => getPreviewStatus(r) === "blocked") ?? false,
    [importPreview],
  );

  return (
    <div className="space-y-6">
      <h2 className="text-lg font-semibold text-text-primary">
        {t("importExport.title", "Role Import / Export")}
      </h2>

      {/* ═══════════ Export Section ═══════════ */}
      <div className="rounded-lg border border-surface-border bg-surface-1 p-4 space-y-3">
        <h3 className="text-sm font-semibold text-text-primary">
          {t("importExport.export.title", "Export Roles")}
        </h3>
        <p className="text-xs text-text-muted">
          {t("importExport.export.selectRoles", "Select roles to export")}
        </p>

        {loading ? (
          <div className="h-20 flex items-center justify-center">
            <div className="h-5 w-5 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
          </div>
        ) : (
          <>
            <div className="max-h-60 overflow-y-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-10" />
                    <TableHead>{t("roles.fields.name", "Name")}</TableHead>
                    <TableHead>{t("roles.fields.description", "Description")}</TableHead>
                    <TableHead className="text-center">
                      {t("users.columns.roles", "Permissions")}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {roles.map((role) => (
                    <TableRow key={role.id}>
                      <TableCell>
                        <Checkbox
                          checked={selectedExport.has(role.id)}
                          onCheckedChange={() => toggleExportRole(role.id)}
                        />
                      </TableCell>
                      <TableCell className="font-medium">{role.name}</TableCell>
                      <TableCell className="text-xs text-text-muted">
                        {role.description ?? "—"}
                      </TableCell>
                      <TableCell className="text-center">
                        <Badge variant="secondary" className="text-[10px]">
                          {role.permissions.length}
                        </Badge>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            <Button
              size="sm"
              disabled={selectedExport.size === 0}
              onClick={() => void handleExport()}
            >
              <Download className="mr-1.5 h-4 w-4" />
              {t("importExport.export.button", "Export JSON")}
            </Button>
          </>
        )}
      </div>

      <Separator />

      {/* ═══════════ Import Section ═══════════ */}
      <div className="rounded-lg border border-surface-border bg-surface-1 p-4 space-y-3">
        <h3 className="text-sm font-semibold text-text-primary">
          {t("importExport.import.title", "Import Roles")}
        </h3>

        <div className="flex items-center gap-3">
          <input
            ref={fileInputRef}
            type="file"
            accept=".json"
            className="hidden"
            onChange={(e) => void handleFileSelect(e)}
          />
          <Button variant="outline" size="sm" onClick={() => fileInputRef.current?.click()}>
            <Upload className="mr-1.5 h-4 w-4" />
            {t("importExport.import.selectFile", "Choose a JSON file")}
          </Button>
        </div>

        {/* Preview table */}
        {importPreview && importPreview.length > 0 && !importResult && (
          <div className="space-y-3">
            <h4 className="text-sm font-medium text-text-primary">
              {t("importExport.import.preview", "Import Preview")}
            </h4>
            <div className="max-h-72 overflow-y-auto rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("importExport.import.columns.name", "Role Name")}</TableHead>
                    <TableHead>
                      {t("importExport.import.columns.permissions", "Permissions")}
                    </TableHead>
                    <TableHead>{t("importExport.import.columns.warnings", "Warnings")}</TableHead>
                    <TableHead>{t("importExport.import.columns.status", "Status")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {importPreview.map((row, idx) => {
                    const status = getPreviewStatus(row);
                    return (
                      <TableRow
                        key={idx}
                        className={status === "blocked" ? "bg-red-50/50 dark:bg-red-950/20" : ""}
                      >
                        <TableCell className="font-medium">{row.entry.name}</TableCell>
                        <TableCell>
                          <Badge variant="secondary" className="text-[10px]">
                            {row.entry.permissions.length}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          {row.validating ? (
                            <span className="text-xs text-text-muted">…</span>
                          ) : status === "blocked" ? (
                            <div className="space-y-0.5">
                              {row.validation?.missing_hard_deps.map((d) => (
                                <div
                                  key={`${d.permission_name}-${d.required_permission_name}`}
                                  className="flex items-center gap-1 text-xs text-destructive"
                                >
                                  <AlertTriangle className="h-3 w-3" />
                                  <strong>{d.permission_name}</strong> →{" "}
                                  {d.required_permission_name}
                                </div>
                              ))}
                              {row.validation?.unknown_permissions.map((p) => (
                                <div
                                  key={p}
                                  className="flex items-center gap-1 text-xs text-destructive"
                                >
                                  <XCircle className="h-3 w-3" />
                                  {p}
                                </div>
                              ))}
                            </div>
                          ) : status === "warnings" ? (
                            <div className="space-y-0.5">
                              {row.validation?.warn_deps.map((d) => (
                                <div
                                  key={`${d.permission_name}-${d.required_permission_name}`}
                                  className="flex items-center gap-1 text-xs text-orange-600"
                                >
                                  <AlertTriangle className="h-3 w-3" />
                                  <strong>{d.permission_name}</strong> →{" "}
                                  {d.required_permission_name}
                                </div>
                              ))}
                            </div>
                          ) : (
                            <span className="text-xs text-text-muted">—</span>
                          )}
                        </TableCell>
                        <TableCell>
                          {row.validating ? (
                            <div className="h-4 w-16 animate-pulse rounded bg-muted" />
                          ) : status === "valid" ? (
                            <Badge variant="default" className="text-[10px]">
                              <CheckCircle className="mr-1 h-3 w-3" />
                              {t("importExport.import.badges.valid", "Valid")}
                            </Badge>
                          ) : status === "warnings" ? (
                            <Badge
                              variant="outline"
                              className="border-orange-300 text-orange-600 text-[10px]"
                            >
                              <AlertTriangle className="mr-1 h-3 w-3" />
                              {t("importExport.import.badges.hasWarnings", "Warnings")}
                            </Badge>
                          ) : status === "blocked" ? (
                            <Badge variant="destructive" className="text-[10px]">
                              <XCircle className="mr-1 h-3 w-3" />
                              {t("importExport.import.badges.blocked", "Blocked")}
                            </Badge>
                          ) : null}
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>

            {hasBlockedRoles && (
              <div className="rounded-md border border-orange-300 bg-orange-50 p-3 text-xs text-orange-800 dark:bg-orange-950/30 dark:text-orange-200">
                <AlertTriangle className="mr-1.5 inline h-3.5 w-3.5" />
                {t("importExport.import.skipped", "{{count}} role(s) skipped", {
                  count: importPreview.filter((r) => getPreviewStatus(r) === "blocked").length,
                })}
              </div>
            )}

            {can("adm.roles") && (
              <Button
                size="sm"
                disabled={importing || importPreview.some((r) => r.validating)}
                onClick={() => void handleImport()}
              >
                {t("importExport.import.button", "Confirm Import")}
              </Button>
            )}
          </div>
        )}

        {/* Import result */}
        {importResult && (
          <Dialog open={!!importResult} onOpenChange={(v) => !v && setImportResult(null)}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>{t("importExport.import.result", "Import Result")}</DialogTitle>
                <DialogDescription>
                  {t("importExport.import.success", "Import complete: {{count}} role(s) imported", {
                    count: importResult.imported_count,
                  })}
                </DialogDescription>
              </DialogHeader>
              {importResult.skipped.length > 0 && (
                <div className="space-y-2">
                  <p className="text-sm font-medium text-destructive">
                    {t("importExport.import.skipped", "{{count}} role(s) skipped", {
                      count: importResult.skipped.length,
                    })}
                  </p>
                  <div className="max-h-40 overflow-y-auto space-y-1">
                    {importResult.skipped.map((s) => (
                      <div key={s.name} className="rounded border p-2 text-xs">
                        <strong>{s.name}</strong>
                        <ul className="mt-0.5 list-inside list-disc text-destructive">
                          {s.errors.map((e, i) => (
                            <li key={i}>{e}</li>
                          ))}
                        </ul>
                      </div>
                    ))}
                  </div>
                </div>
              )}
              <DialogFooter>
                <Button
                  onClick={() => {
                    setImportResult(null);
                    setImportPreview(null);
                  }}
                >
                  {t("common.close", "Close")}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        )}
      </div>

      {StepUpDialogElement}
    </div>
  );
}

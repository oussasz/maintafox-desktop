/**
 * Document library — one category (technical manuals, SOPs, safety protocols, certificates).
 *
 * OT/DI pattern: header (title · count · [Add document] [Refresh]), one search box, DataTable,
 * LoadingState / ErrorState with retry / EmptyState. Adding a document is an explicit dialog
 * (title, file, optional equipment) — nothing is uploaded until "Upload" is pressed. Deleting asks
 * first; downloads go through the native Save dialog.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Download, FilePlus2, Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, Navigate, useParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetPicker } from "@/components/assets/AssetPicker";
import { DataTable } from "@/components/data/DataTable";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import {
  MediaFileStateBadge,
  isMediaFileAvailable,
  useMediaFileStates,
} from "@/components/sync/MediaFileStatus";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { mfLayout } from "@/design-system/tokens";
import { saveBlobWithDialog } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { assetToSearchResult } from "@/lib/asset-to-search-result";
import { getAssetByIdSilent } from "@/services/asset-service";
import {
  MAX_LIBRARY_DOCUMENT_BYTES,
  deleteLibraryDocument,
  fileToNumberArray,
  getLibraryDocumentFile,
  listLibraryDocuments,
  updateLibraryDocument,
  uploadLibraryDocument,
} from "@/services/documentation-service";
import { pushAppToast } from "@/store/app-toast-store";
import type {
  AssetSearchResult,
  LibraryDocument,
  LibraryDocumentCategory,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { SLUG_TO_CATEGORY, isDocumentationCategorySlug } from "./documentation-slugs";

const MAX_MB = Math.floor(MAX_LIBRARY_DOCUMENT_BYTES / (1024 * 1024));

/** "manual.pdf" → "manual" (suggested title). */
function baseName(fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  return dot > 0 ? fileName.slice(0, dot) : fileName;
}

function extensionOf(fileName: string): string | null {
  const dot = fileName.lastIndexOf(".");
  return dot > 0 && dot < fileName.length - 1 ? fileName.slice(dot + 1).toLowerCase() : null;
}

// ── Upload dialog ─────────────────────────────────────────────────────────────

interface UploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  category: LibraryDocumentCategory;
  onUploaded: () => void;
}

function DocumentUploadDialog({ open, onOpenChange, category, onUploaded }: UploadDialogProps) {
  const { t } = useTranslation("documentation");
  const [file, setFile] = useState<File | null>(null);
  const [title, setTitle] = useState("");
  const [notes, setNotes] = useState("");
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const [fileInputKey, setFileInputKey] = useState(0);

  useEffect(() => {
    if (!open) return;
    setFile(null);
    setTitle("");
    setNotes("");
    setAsset(null);
    setFileError(null);
    setFileInputKey((k) => k + 1);
  }, [open]);

  const pickFile = (files: FileList | null) => {
    const picked = files?.[0] ?? null;
    setFileError(null);
    if (picked && picked.size > MAX_LIBRARY_DOCUMENT_BYTES) {
      setFile(null);
      setFileError(t("errors.fileTooLarge", { mb: MAX_MB }));
      setFileInputKey((k) => k + 1);
      return;
    }
    setFile(picked);
    if (picked && !title.trim()) setTitle(baseName(picked.name));
  };

  const canUpload = file != null && title.trim().length > 0 && !uploading;

  const upload = async () => {
    if (!file || !title.trim()) return;
    setUploading(true);
    try {
      await uploadLibraryDocument({
        category,
        equipmentId: asset?.id ?? null,
        title: title.trim(),
        fileName: file.name,
        fileBytes: await fileToNumberArray(file),
        mimeType: file.type || "application/octet-stream",
        notes: notes.trim() ? notes.trim() : null,
      });
      pushAppToast({ title: t("upload.done", { title: title.trim() }), variant: "success" });
      onOpenChange(false);
      onUploaded();
    } catch {
      // The global IPC toast reports the failure; the dialog keeps the user's input.
    } finally {
      setUploading(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !uploading && onOpenChange(o)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("upload.title")}</DialogTitle>
          <DialogDescription>{t("upload.maxSize", { mb: MAX_MB })}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-1">
          <div className="grid gap-1.5">
            <Label htmlFor="doc-file">{t("upload.fields.file")}</Label>
            <input
              key={fileInputKey}
              id="doc-file"
              type="file"
              aria-describedby={fileError ? "doc-file-error" : undefined}
              aria-invalid={fileError ? true : undefined}
              className="min-h-10 text-sm text-text-secondary file:me-3 file:h-10 file:cursor-pointer file:rounded-md file:border file:border-surface-border file:bg-surface-1 file:px-3 file:text-sm file:font-medium file:text-text-primary hover:file:bg-surface-2"
              disabled={uploading}
              onChange={(e) => pickFile(e.target.files)}
            />
            {fileError ? (
              <p id="doc-file-error" role="alert" className="text-xs text-tone-danger-fg">
                {fileError}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-title">
              {t("upload.fields.title")} <span aria-hidden>*</span>
            </Label>
            <Input
              id="doc-title"
              value={title}
              required
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t("upload.fields.titlePlaceholder")}
              disabled={uploading}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-equipment">{t("upload.fields.equipment")}</Label>
            <AssetPicker
              id="doc-equipment"
              value={asset}
              onChange={setAsset}
              disabled={uploading}
            />
            <p className="text-xs text-text-muted">{t("upload.fields.equipmentHint")}</p>
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-notes">{t("upload.fields.notes")}</Label>
            <Input
              id="doc-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t("upload.fields.notesPlaceholder")}
              disabled={uploading}
            />
          </div>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={uploading}
          >
            {t("edit.cancel")}
          </Button>
          <Button className="h-11" onClick={() => void upload()} disabled={!canUpload}>
            {uploading ? t("upload.uploading") : t("upload.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Edit dialog ───────────────────────────────────────────────────────────────

interface EditDialogProps {
  doc: LibraryDocument | null;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}

function DocumentEditDialog({ doc, onOpenChange, onSaved }: EditDialogProps) {
  const { t } = useTranslation("documentation");
  const [title, setTitle] = useState("");
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [assetLoading, setAssetLoading] = useState(false);
  const [assetMissing, setAssetMissing] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!doc) return;
    let cancelled = false;
    setTitle(doc.title);
    setAsset(null);
    setAssetMissing(false);
    if (doc.equipmentId == null) {
      setAssetLoading(false);
      return;
    }
    setAssetLoading(true);
    void getAssetByIdSilent(doc.equipmentId)
      .then((row) => {
        if (cancelled) return;
        if (row) setAsset(assetToSearchResult(row));
        else setAssetMissing(true);
      })
      .catch(() => {
        if (!cancelled) setAssetMissing(true);
      })
      .finally(() => {
        if (!cancelled) setAssetLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [doc]);

  const save = async () => {
    if (!doc || !title.trim()) return;
    setSaving(true);
    try {
      // When the current link could not be read, keep it unless the user picked another one.
      const keepUnreadLink = assetMissing && asset == null;
      await updateLibraryDocument({
        id: doc.id,
        title: title.trim(),
        clearEquipmentLink: !keepUnreadLink && asset == null,
        equipmentId: keepUnreadLink ? doc.equipmentId : (asset?.id ?? null),
      });
      onOpenChange(false);
      onSaved();
    } catch {
      // The global IPC toast reports the failure.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={doc != null} onOpenChange={(o) => !saving && onOpenChange(o)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("edit.title")}</DialogTitle>
          <DialogDescription>{doc?.fileName ?? ""}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-1">
          <div className="grid gap-1.5">
            <Label htmlFor="doc-edit-title">
              {t("upload.fields.title")} <span aria-hidden>*</span>
            </Label>
            <Input
              id="doc-edit-title"
              value={title}
              required
              onChange={(e) => setTitle(e.target.value)}
              disabled={saving}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-edit-equipment">{t("upload.fields.equipment")}</Label>
            {assetLoading ? (
              <LoadingState className="py-3" />
            ) : (
              <AssetPicker
                id="doc-edit-equipment"
                value={asset}
                onChange={(next) => {
                  setAsset(next);
                  setAssetMissing(false);
                }}
                disabled={saving}
              />
            )}
            {assetMissing && doc?.equipmentCode ? (
              <p className="text-xs text-tone-warning-fg" role="status">
                {t("edit.linkUnreadable", { code: doc.equipmentCode })}
              </p>
            ) : null}
          </div>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("edit.cancel")}
          </Button>
          <Button
            className="h-11"
            onClick={() => void save()}
            disabled={saving || assetLoading || !title.trim()}
          >
            {saving ? t("edit.saving") : t("edit.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Page ──────────────────────────────────────────────────────────────────────

export function DocumentationCategoryPage() {
  const { t } = useTranslation("documentation");
  const { formatDateTime, formatDecimal, formatNumber } = useFormatters();
  const { categorySlug } = useParams<{ categorySlug: string }>();
  const { can } = usePermissions();

  const category = useMemo((): LibraryDocumentCategory | null => {
    if (!isDocumentationCategorySlug(categorySlug)) return null;
    return SLUG_TO_CATEGORY[categorySlug];
  }, [categorySlug]);

  const canManage = can(P.DOC_MANAGE);

  const [docs, setDocs] = useState<LibraryDocument[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [editing, setEditing] = useState<LibraryDocument | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<LibraryDocument | null>(null);
  const [downloadingId, setDownloadingId] = useState<number | null>(null);

  const { states: fileStates, refresh: refreshFileStates } = useMediaFileStates(
    docs.map((d) => d.relativePath),
  );

  const load = useCallback(async () => {
    if (!category) return;
    setLoading(true);
    setLoadError(null);
    try {
      setDocs(await listLibraryDocuments(category, null));
    } catch (e) {
      setLoadError(e);
    } finally {
      setLoading(false);
    }
  }, [category]);

  useEffect(() => {
    setDocs([]);
    void load();
  }, [load]);

  const formatSize = useCallback(
    (bytes: number) => {
      if (bytes < 1024) return t("size.bytes", { value: formatNumber(bytes) });
      if (bytes < 1024 * 1024) return t("size.kb", { value: formatDecimal(bytes / 1024, 1) });
      return t("size.mb", { value: formatDecimal(bytes / (1024 * 1024), 1) });
    },
    [t, formatNumber, formatDecimal],
  );

  const handleDownload = useCallback(
    async (doc: LibraryDocument) => {
      setDownloadingId(doc.id);
      try {
        let bytes: Uint8Array;
        try {
          bytes = await getLibraryDocumentFile(doc.id);
        } catch {
          return; // The IPC layer already reported the failure.
        }
        const copy = new Uint8Array(bytes.byteLength);
        copy.set(bytes);
        const blob = new Blob([copy], { type: doc.mimeType || "application/octet-stream" });
        const ext = extensionOf(doc.fileName);
        try {
          const path = await saveBlobWithDialog(blob, doc.fileName, {
            title: t("list.downloadTitle", { title: doc.title }),
            filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : [],
          });
          pushAppToast({ title: t("list.downloaded"), description: path, variant: "success" });
        } catch (e) {
          if (isExportCancelled(e)) return;
          // Writing the file on this computer failed (not an IPC call: no global toast).
          pushAppToast({
            title: t("list.downloadFailed"),
            description: describeAppError(e).title,
            variant: "destructive",
          });
        }
      } finally {
        setDownloadingId(null);
      }
    },
    [t],
  );

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    await deleteLibraryDocument(deleteTarget.id);
    await load();
  }, [deleteTarget, load]);

  const columns = useMemo<ColumnDef<LibraryDocument>[]>(
    () => [
      {
        accessorKey: "title",
        header: t("list.columns.title"),
        cell: ({ row }) => (
          <div className="min-w-0">
            <div className="font-medium text-text-primary">{row.original.title}</div>
            {row.original.notes ? (
              <div className="truncate text-xs text-text-muted">{row.original.notes}</div>
            ) : null}
          </div>
        ),
      },
      {
        id: "file",
        accessorFn: (d) => d.fileName,
        header: t("list.columns.file"),
        cell: ({ row }) => (
          <div className="text-text-secondary">
            <div className="break-all">{row.original.fileName}</div>
            <div className="text-xs text-text-muted">{formatSize(row.original.sizeBytes)}</div>
            <MediaFileStateBadge
              state={fileStates.get(row.original.relativePath)}
              onRetried={() => void refreshFileStates()}
            />
          </div>
        ),
      },
      {
        id: "equipment",
        accessorFn: (d) => `${d.equipmentCode ?? ""} ${d.equipmentName ?? ""}`.trim(),
        header: t("list.columns.equipment"),
        cell: ({ row }) =>
          row.original.equipmentId != null && row.original.equipmentCode ? (
            <Link
              to={`/equipment?assetId=${row.original.equipmentId}`}
              className="rounded text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              title={t("list.openEquipment", { code: row.original.equipmentCode })}
            >
              <span className="font-mono text-xs">{row.original.equipmentCode}</span>
              {row.original.equipmentName ? (
                <span className="ms-1 text-xs text-text-secondary">
                  {row.original.equipmentName}
                </span>
              ) : null}
            </Link>
          ) : (
            <span className="text-text-muted">{t("upload.fields.noEquipment")}</span>
          ),
      },
      {
        accessorKey: "uploadedAt",
        header: t("list.columns.uploaded"),
        cell: ({ row }) => (
          <span className="whitespace-nowrap text-xs text-text-secondary">
            {formatDateTime(row.original.uploadedAt)}
          </span>
        ),
      },
      {
        id: "actions",
        enableSorting: false,
        header: () => <span className="sr-only">{t("list.columns.actions")}</span>,
        cell: ({ row }) => {
          const doc = row.original;
          const available = isMediaFileAvailable(fileStates.get(doc.relativePath));
          return (
            <div className="flex justify-end gap-1">
              <Button
                type="button"
                variant="ghost"
                className="h-10 w-10 p-0"
                aria-label={t("list.downloadNamed", { title: doc.title })}
                title={available ? t("list.download") : t("list.downloadUnavailable")}
                disabled={!available || downloadingId === doc.id}
                onClick={() => void handleDownload(doc)}
              >
                <Download className="h-4 w-4" aria-hidden />
              </Button>
              {canManage ? (
                <>
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-10 w-10 p-0"
                    aria-label={t("list.editNamed", { title: doc.title })}
                    title={t("list.edit")}
                    onClick={() => setEditing(doc)}
                  >
                    <Pencil className="h-4 w-4" aria-hidden />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-10 w-10 p-0"
                    aria-label={t("list.deleteNamed", { title: doc.title })}
                    title={t("list.delete")}
                    onClick={() => setDeleteTarget(doc)}
                  >
                    <Trash2 className="h-4 w-4 text-tone-danger-fg" aria-hidden />
                  </Button>
                </>
              ) : null}
            </div>
          );
        },
      },
    ],
    [
      t,
      formatSize,
      formatDateTime,
      fileStates,
      refreshFileStates,
      canManage,
      downloadingId,
      handleDownload,
    ],
  );

  if (!category || !categorySlug || !isDocumentationCategorySlug(categorySlug)) {
    return <Navigate to="/documentation/technical-manuals" replace />;
  }

  return (
    <div className="flex min-h-full flex-col gap-4 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 max-w-3xl">
          <div className="flex items-center gap-2">
            <h2 className="text-lg font-semibold text-text-primary">
              {t(`categories.${categorySlug}.heading`)}
            </h2>
            {!loading && loadError == null ? (
              <Badge variant="secondary" className={mfLayout.moduleCount}>
                {formatNumber(docs.length)}
              </Badge>
            ) : null}
          </div>
          <p className="mt-0.5 text-sm text-text-muted">
            {t(`categories.${categorySlug}.description`)}
          </p>
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          <PermissionGate permission={P.DOC_MANAGE}>
            <Button className={mfLayout.primaryAction} onClick={() => setUploadOpen(true)}>
              <FilePlus2 className="h-4 w-4" aria-hidden />
              {t("upload.open")}
            </Button>
          </PermissionGate>
          <RefreshButton onRefresh={() => void load()} loading={loading} />
        </div>
      </div>

      {loading && docs.length === 0 ? (
        <LoadingState label={t("list.loading")} />
      ) : loadError != null ? (
        <ErrorState error={loadError} title={t("list.loadError")} onRetry={() => void load()} />
      ) : docs.length === 0 ? (
        <EmptyState
          title={t("list.empty")}
          {...(canManage
            ? {
                description: t("list.emptyHint"),
                actionLabel: t("upload.open"),
                onAction: () => setUploadOpen(true),
              }
            : {})}
        />
      ) : (
        <DataTable
          columns={columns}
          data={docs}
          searchable
          searchPlaceholder={t("list.searchPlaceholder")}
          pageSize={25}
        />
      )}

      <DocumentUploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        category={category}
        onUploaded={() => void load()}
      />
      <DocumentEditDialog
        doc={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        onSaved={() => void load()}
      />
      <ConfirmDialog
        open={deleteTarget != null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title={t("confirm.deleteTitle")}
        description={t("confirm.delete", { title: deleteTarget?.title ?? "" })}
        confirmLabel={t("confirm.deleteAction")}
        destructive
        onConfirm={confirmDelete}
      />
    </div>
  );
}

/**
 * Document library — one category of the plant's list (Données de référence, `DOCUMENT.CATEGORIES`).
 *
 * OT/DI pattern: header (title · count · [Add document] [Refresh]), one search box, DataTable,
 * LoadingState / ErrorState with retry / EmptyState. Adding a document is an explicit dialog
 * (file, title, category shown and changeable, reference number, revision, validity date, optional
 * equipment) — nothing is uploaded until "Upload" is pressed. Expired documents and those expiring
 * within 30 days are flagged in the list and counted above it (L7-18). Deleting asks first;
 * downloads go through the native Save dialog.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Download, FilePlus2, FileUp, History, Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, Navigate, useParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetPicker } from "@/components/assets/AssetPicker";
import { DataTable } from "@/components/data/DataTable";
import {
  DocumentHistoryDialog,
  DocumentNewVersionDialog,
} from "@/components/documentation/DocumentVersionDialogs";
import {
  EXPIRY_WARNING_DAYS,
  documentValidity,
} from "@/components/documentation/document-validity";
import {
  type DocumentCategories,
  categoryFromSlug,
  documentCategoryPath,
  useDocumentCategories,
} from "@/components/documentation/use-document-categories";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import {
  MediaFileStateBadge,
  isMediaFileAvailable,
  useMediaFileStates,
} from "@/components/sync/MediaFileStatus";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { FileInput } from "@/components/ui/file-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ReviewMark } from "@/components/ui/review-mark";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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

// ── Document control fields (category, number, revision, validity) ─────────────

interface ControlValues {
  category: LibraryDocumentCategory;
  documentNumber: string;
  revision: string;
  validUntil: string;
  /** The typed validity text is not a date yet: saving is refused, never stored as "no date". */
  validUntilInvalid: boolean;
}

const EMPTY_CONTROL = {
  documentNumber: "",
  revision: "",
  validUntil: "",
  validUntilInvalid: false,
};

interface ControlFieldsProps {
  idPrefix: string;
  values: ControlValues;
  onChange: (next: ControlValues) => void;
  categories: DocumentCategories;
  disabled: boolean;
}

function DocumentControlFields({
  idPrefix,
  values,
  onChange,
  categories,
  disabled,
}: ControlFieldsProps) {
  const { t } = useTranslation("documentation");
  const set = (patch: Partial<ControlValues>) => onChange({ ...values, ...patch });
  // A category no longer in the list stays shown (and kept) until another one is chosen.
  const listed = categories.values.some((c) => c.code === values.category);
  return (
    <>
      <div className="grid gap-1.5">
        <Label htmlFor={`${idPrefix}-category`}>
          {t("upload.fields.category")} <span aria-hidden>*</span>
        </Label>
        <Select
          value={values.category}
          onValueChange={(v) => set({ category: v })}
          disabled={disabled}
        >
          <SelectTrigger id={`${idPrefix}-category`} className="h-10">
            <SelectValue>{categories.text(values.category)}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            {!listed && values.category ? (
              <SelectItem value={values.category}>{categories.text(values.category)}</SelectItem>
            ) : null}
            {categories.values.map((c) => (
              <SelectItem key={c.code} value={c.code}>
                {categories.text(c.code)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-1.5">
          <Label htmlFor={`${idPrefix}-number`}>{t("upload.fields.documentNumber")}</Label>
          <Input
            id={`${idPrefix}-number`}
            value={values.documentNumber}
            maxLength={64}
            onChange={(e) => set({ documentNumber: e.target.value })}
            placeholder={t("upload.fields.documentNumberPlaceholder")}
            disabled={disabled}
          />
        </div>
        <div className="grid gap-1.5">
          <Label htmlFor={`${idPrefix}-revision`}>{t("upload.fields.revision")}</Label>
          <Input
            id={`${idPrefix}-revision`}
            value={values.revision}
            maxLength={64}
            onChange={(e) => set({ revision: e.target.value })}
            placeholder={t("upload.fields.revisionPlaceholder")}
            disabled={disabled}
          />
        </div>
      </div>
      <div className="grid gap-1.5">
        <Label htmlFor={`${idPrefix}-valid-until`}>{t("upload.fields.validUntil")}</Label>
        <DateField
          id={`${idPrefix}-valid-until`}
          value={values.validUntil}
          onChange={(v) => set({ validUntil: v })}
          onInvalidChange={(invalid) => {
            if (invalid !== values.validUntilInvalid) set({ validUntilInvalid: invalid });
          }}
          disabled={disabled}
          aria-invalid={values.validUntilInvalid || undefined}
          aria-describedby={`${idPrefix}-valid-until-hint`}
        />
        {values.validUntilInvalid ? (
          <p
            id={`${idPrefix}-valid-until-hint`}
            role="alert"
            className="text-xs text-tone-danger-fg"
          >
            {t("upload.fields.validUntilInvalid")}
          </p>
        ) : (
          <p id={`${idPrefix}-valid-until-hint`} className="text-xs text-text-muted">
            {t("upload.fields.validUntilHint", { days: EXPIRY_WARNING_DAYS })}
          </p>
        )}
      </div>
    </>
  );
}

/** Blank → null (the backend stores nothing for an empty field). */
function orNull(value: string): string | null {
  const v = value.trim();
  return v ? v : null;
}

// ── Upload dialog ─────────────────────────────────────────────────────────────

interface UploadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  category: LibraryDocumentCategory;
  categories: DocumentCategories;
  onUploaded: () => void;
}

function DocumentUploadDialog({
  open,
  onOpenChange,
  category,
  categories,
  onUploaded,
}: UploadDialogProps) {
  const { t } = useTranslation("documentation");
  const [control, setControl] = useState<ControlValues>({ category, ...EMPTY_CONTROL });
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
    // The category of the open tab is proposed, shown and changeable.
    setControl({ category, ...EMPTY_CONTROL });
  }, [open, category]);

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

  const canUpload =
    file != null && title.trim().length > 0 && !control.validUntilInvalid && !uploading;

  const upload = async () => {
    if (!file || !title.trim() || control.validUntilInvalid) return;
    setUploading(true);
    try {
      await uploadLibraryDocument({
        category: control.category,
        equipmentId: asset?.id ?? null,
        title: title.trim(),
        fileName: file.name,
        fileBytes: await fileToNumberArray(file),
        mimeType: file.type || "application/octet-stream",
        notes: notes.trim() ? notes.trim() : null,
        documentNumber: orNull(control.documentNumber),
        revision: orNull(control.revision),
        validUntil: orNull(control.validUntil),
      });
      pushAppToast({
        title:
          control.category === category
            ? t("upload.done", { title: title.trim() })
            : t("upload.doneIn", {
                title: title.trim(),
                category: categories.text(control.category),
              }),
        variant: "success",
      });
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
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("upload.title")}</DialogTitle>
          <DialogDescription>{t("upload.maxSize", { mb: MAX_MB })}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-1">
          <div className="grid gap-1.5">
            <Label htmlFor="doc-file">{t("upload.fields.file")}</Label>
            <FileInput
              key={fileInputKey}
              id="doc-file"
              aria-describedby={fileError ? "doc-file-error" : undefined}
              aria-invalid={fileError ? true : undefined}
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
          <DocumentControlFields
            idPrefix="doc"
            values={control}
            onChange={setControl}
            categories={categories}
            disabled={uploading}
          />
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
  categories: DocumentCategories;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}

function DocumentEditDialog({ doc, categories, onOpenChange, onSaved }: EditDialogProps) {
  const { t } = useTranslation("documentation");
  const [title, setTitle] = useState("");
  const [control, setControl] = useState<ControlValues>({ category: "", ...EMPTY_CONTROL });
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [assetLoading, setAssetLoading] = useState(false);
  const [assetMissing, setAssetMissing] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!doc) return;
    let cancelled = false;
    setTitle(doc.title);
    setControl({
      category: doc.category,
      documentNumber: doc.documentNumber ?? "",
      revision: doc.revision ?? "",
      validUntil: doc.validUntil ?? "",
      validUntilInvalid: false,
    });
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
    if (!doc || !title.trim() || control.validUntilInvalid) return;
    setSaving(true);
    try {
      // When the current link could not be read, keep it unless the user picked another one.
      const keepUnreadLink = assetMissing && asset == null;
      await updateLibraryDocument({
        id: doc.id,
        title: title.trim(),
        clearEquipmentLink: !keepUnreadLink && asset == null,
        equipmentId: keepUnreadLink ? doc.equipmentId : (asset?.id ?? null),
        category: control.category,
        // An empty text clears the field.
        documentNumber: control.documentNumber.trim(),
        revision: control.revision.trim(),
        validUntil: control.validUntil,
      });
      if (control.category !== doc.category) {
        pushAppToast({
          title: t("edit.moved", {
            title: title.trim(),
            category: categories.text(control.category),
          }),
          variant: "success",
        });
      }
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
      <DialogContent className="max-w-2xl">
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
          <DocumentControlFields
            idPrefix="doc-edit"
            values={control}
            onChange={setControl}
            categories={categories}
            disabled={saving}
          />
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
            disabled={saving || assetLoading || !title.trim() || control.validUntilInvalid}
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
  const { formatDate, formatDateTime, formatBytes: formatSize, formatNumber } = useFormatters();
  const { categorySlug } = useParams<{ categorySlug: string }>();
  const { can } = usePermissions();
  const categories = useDocumentCategories();

  // The URL names the category by its exact code (older links: the code written with dashes).
  const categoryOption = useMemo(
    () => categoryFromSlug(categories.values, categorySlug),
    [categories.values, categorySlug],
  );
  const category: LibraryDocumentCategory | null = categoryOption?.code ?? null;

  const canManage = can(P.DOC_MANAGE);

  const [docs, setDocs] = useState<LibraryDocument[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [editing, setEditing] = useState<LibraryDocument | null>(null);
  const [newVersionOf, setNewVersionOf] = useState<LibraryDocument | null>(null);
  const [historyOf, setHistoryOf] = useState<LibraryDocument | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<LibraryDocument | null>(null);
  const [downloadingId, setDownloadingId] = useState<number | null>(null);
  // One search box, always shown; "toute la bibliothèque" searches every category at once.
  const [search, setSearch] = useState("");
  const [searchAll, setSearchAll] = useState(false);

  const { states: fileStates, refresh: refreshFileStates } = useMediaFileStates(
    docs.map((d) => d.relativePath),
  );

  const load = useCallback(async () => {
    if (!category) return;
    setLoading(true);
    setLoadError(null);
    try {
      setDocs(await listLibraryDocuments(searchAll ? null : category, null));
    } catch (e) {
      setLoadError(e);
    } finally {
      setLoading(false);
    }
  }, [category, searchAll]);

  useEffect(() => {
    setDocs([]);
    void load();
  }, [load]);

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

  /** The documents matching the search (title, reference, file, equipment, notes). */
  const shown = useMemo(() => {
    const words = search.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return docs;
    return docs.filter((d) => {
      const text = [
        d.title,
        d.documentNumber,
        d.revision,
        d.fileName,
        d.equipmentCode,
        d.equipmentName,
        d.notes,
        searchAll ? categories.text(d.category) : null,
      ]
        .filter(Boolean)
        .join(" ")
        .toLocaleLowerCase();
      return words.every((w) => text.includes(w));
    });
  }, [docs, search, searchAll, categories]);

  // Expiry alert: how many documents of this category have expired or expire soon.
  const expiry = useMemo(() => {
    let expired = 0;
    let soon = 0;
    if (searchAll) return { expired, soon };
    for (const d of docs) {
      const v = documentValidity(d.validUntil);
      if (v === "expired") expired += 1;
      else if (v === "soon") soon += 1;
    }
    return { expired, soon };
  }, [docs, searchAll]);

  const columns = useMemo<ColumnDef<LibraryDocument>[]>(
    () => [
      ...(searchAll
        ? [
            {
              id: "category",
              accessorFn: (d: LibraryDocument) => categories.text(d.category),
              header: t("list.columns.category"),
              cell: ({ row }: { row: { original: LibraryDocument } }) => (
                <Link
                  to={documentCategoryPath(row.original.category)}
                  className="text-sm text-text-secondary underline-offset-2 hover:underline"
                >
                  {categories.text(row.original.category)}
                </Link>
              ),
            } satisfies ColumnDef<LibraryDocument>,
          ]
        : []),
      {
        id: "title",
        accessorFn: (d) =>
          [d.title, d.documentNumber ?? "", d.revision ?? ""].filter(Boolean).join(" "),
        header: t("list.columns.title"),
        cell: ({ row }) => (
          <div className="min-w-0">
            <div className="font-medium text-text-primary">{row.original.title}</div>
            {row.original.documentNumber || row.original.revision ? (
              <div className="text-xs text-text-secondary">
                {row.original.documentNumber ? (
                  <span className="font-mono">{row.original.documentNumber}</span>
                ) : null}
                {row.original.documentNumber && row.original.revision ? " · " : null}
                {row.original.revision
                  ? t("list.revisionLabel", { revision: row.original.revision })
                  : null}
              </div>
            ) : null}
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
        id: "validity",
        accessorFn: (d) => d.validUntil ?? "9999-12-31",
        header: t("list.columns.validity"),
        cell: ({ row }) => {
          const until = row.original.validUntil;
          const validity = documentValidity(until);
          if (validity == null || until == null) {
            return <span className="text-xs text-text-muted">{t("list.validity.none")}</span>;
          }
          const date = formatDate(until);
          if (validity === "expired") {
            return (
              <Badge variant="destructive" className="whitespace-nowrap">
                {t("list.validity.expired", { date })}
              </Badge>
            );
          }
          if (validity === "soon") {
            return (
              <Badge
                variant="outline"
                className="whitespace-nowrap border-tone-warning-border bg-tone-warning-bg text-tone-warning-fg"
              >
                {t("list.validity.soon", { date })}
              </Badge>
            );
          }
          return (
            <span className="whitespace-nowrap text-xs text-text-secondary">
              {t("list.validity.until", { date })}
            </span>
          );
        },
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
              {doc.previousVersionId != null ? (
                <Button
                  type="button"
                  variant="ghost"
                  className="h-10 w-10 p-0"
                  aria-label={t("versions.historyNamed", { title: doc.title })}
                  title={t("versions.history")}
                  onClick={() => setHistoryOf(doc)}
                >
                  <History className="h-4 w-4" aria-hidden />
                </Button>
              ) : null}
              {canManage ? (
                <>
                  <Button
                    type="button"
                    variant="ghost"
                    className="h-10 w-10 p-0"
                    aria-label={t("versions.newNamed", { title: doc.title })}
                    title={t("versions.new")}
                    onClick={() => setNewVersionOf(doc)}
                  >
                    <FileUp className="h-4 w-4" aria-hidden />
                  </Button>
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
      searchAll,
      categories,
      formatSize,
      formatDate,
      formatDateTime,
      fileStates,
      refreshFileStates,
      canManage,
      downloadingId,
      handleDownload,
    ],
  );

  // Without the right, or while the list fails, the library's side panel says why.
  if (!categories.canRead || categories.error != null) return null;
  if (!categories.loaded) return <LoadingState />;
  if (!category) {
    // A category no longer in the list (or a mistyped link): back to the library's first one.
    return <Navigate to="/documentation" replace />;
  }
  if (categorySlug !== category) {
    return <Navigate to={documentCategoryPath(category)} replace />;
  }

  return (
    <div className="flex min-h-full flex-col gap-4 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 max-w-3xl">
          <div className="flex items-center gap-2">
            <h2 className="text-lg font-semibold text-text-primary">{categories.text(category)}</h2>
            {categoryOption?.review_required ? <ReviewMark /> : null}
            {!loading && loadError == null ? (
              <Badge variant="secondary" className={mfLayout.moduleCount}>
                {formatNumber(shown.length)}
              </Badge>
            ) : null}
          </div>
          <p className="mt-0.5 text-sm text-text-muted">
            {t("category.description", { name: categories.text(category) })}
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

      {!loading && loadError == null && (expiry.expired > 0 || expiry.soon > 0) ? (
        <div
          role="status"
          className="rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm text-tone-warning-fg"
        >
          {[
            expiry.expired > 0 ? t("expiry.expired", { count: expiry.expired }) : null,
            expiry.soon > 0
              ? t("expiry.soon", { count: expiry.soon, days: EXPIRY_WARNING_DAYS })
              : null,
          ]
            .filter(Boolean)
            .join(" · ")}
        </div>
      ) : null}

      <div className="flex flex-wrap items-center gap-3">
        <Input
          type="search"
          className="max-w-sm"
          placeholder={t("list.searchPlaceholder")}
          aria-label={t("list.searchPlaceholder")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <label className="flex items-center gap-2 text-sm text-text-secondary">
          <Checkbox
            checked={searchAll}
            onCheckedChange={(c) => setSearchAll(Boolean(c))}
            data-testid="doc-search-all"
          />
          {t("list.searchAll")}
        </label>
      </div>

      {loading && docs.length === 0 ? (
        <LoadingState label={t("list.loading")} />
      ) : loadError != null ? (
        <ErrorState error={loadError} title={t("list.loadError")} onRetry={() => void load()} />
      ) : docs.length === 0 && search.trim() === "" ? (
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
        <DataTable columns={columns} data={shown} pageSize={25} emptyMessage={t("list.noMatch")} />
      )}

      <DocumentUploadDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        category={category}
        categories={categories}
        onUploaded={() => void load()}
      />
      <DocumentEditDialog
        doc={editing}
        categories={categories}
        onOpenChange={(o) => !o && setEditing(null)}
        onSaved={() => void load()}
      />
      <DocumentNewVersionDialog
        doc={newVersionOf}
        onOpenChange={(o) => !o && setNewVersionOf(null)}
        onUploaded={() => void load()}
      />
      <DocumentHistoryDialog
        doc={historyOf}
        onOpenChange={(o) => !o && setHistoryOf(null)}
        onDownload={(v) => void handleDownload(v)}
      />
      <ConfirmDialog
        open={deleteTarget != null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title={t("confirm.deleteTitle")}
        description={
          deleteTarget?.previousVersionId != null
            ? t("confirm.deleteVersion", {
                title: deleteTarget.title,
                revision: deleteTarget.revision ?? "",
              })
            : t("confirm.delete", { title: deleteTarget?.title ?? "" })
        }
        confirmLabel={t("confirm.deleteAction")}
        destructive
        onConfirm={confirmDelete}
      />
    </div>
  );
}

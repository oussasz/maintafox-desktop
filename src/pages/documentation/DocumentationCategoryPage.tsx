import { Download, Loader2, Pencil, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Navigate, useParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { listAssets } from "@/services/asset-service";
import {
  deleteLibraryDocument,
  fileToNumberArray,
  getLibraryDocumentFile,
  listLibraryDocuments,
  MAX_LIBRARY_DOCUMENT_BYTES,
  updateLibraryDocument,
  uploadLibraryDocument,
} from "@/services/documentation-service";
import { toErrorMessage } from "@/utils/errors";
import type { Asset, LibraryDocument, LibraryDocumentCategory } from "@shared/ipc-types";

import { isDocumentationCategorySlug, SLUG_TO_CATEGORY } from "./documentation-slugs";

export function DocumentationCategoryPage() {
  const { t, i18n } = useTranslation("documentation");
  const { categorySlug } = useParams<{ categorySlug: string }>();
  const { can } = usePermissions();

  const category = useMemo((): LibraryDocumentCategory | null => {
    if (!isDocumentationCategorySlug(categorySlug)) return null;
    return SLUG_TO_CATEGORY[categorySlug];
  }, [categorySlug]);

  const canManage = can("doc.manage");

  const [docs, setDocs] = useState<LibraryDocument[]>([]);
  const [assets, setAssets] = useState<Asset[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [title, setTitle] = useState("");
  const [equipmentId, setEquipmentId] = useState<string>("__none__");
  const [notes, setNotes] = useState("");
  const [uploading, setUploading] = useState(false);
  const [fileInputKey, setFileInputKey] = useState(0);

  const [editOpen, setEditOpen] = useState(false);
  const [editing, setEditing] = useState<LibraryDocument | null>(null);
  const [editTitle, setEditTitle] = useState("");
  const [editEquipmentId, setEditEquipmentId] = useState<string>("__none__");
  const [savingEdit, setSavingEdit] = useState(false);

  const load = useCallback(async () => {
    if (!category) return;
    setLoading(true);
    setError(null);
    try {
      const rows = await listLibraryDocuments(category, null);
      setDocs(rows);
    } catch (e) {
      setError(toErrorMessage(e));
      setDocs([]);
    } finally {
      setLoading(false);
    }
  }, [category]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const rows = await listAssets(null, null, null, 3000);
        if (!cancelled) {
          setAssets(rows.filter((a) => a.deleted_at == null));
        }
      } catch {
        if (!cancelled) setAssets([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleUpload = useCallback(
    async (files: FileList | null) => {
      if (!category || !files?.length) return;
      const file = files[0];
      if (!file) return;
      if (file.size > MAX_LIBRARY_DOCUMENT_BYTES) {
        setError(
          t("errors.fileTooLarge", { mb: Math.floor(MAX_LIBRARY_DOCUMENT_BYTES / (1024 * 1024)) }),
        );
        return;
      }
      setError(null);
      setUploading(true);
      try {
        const bytes = await fileToNumberArray(file);
        const eq = equipmentId === "__none__" ? null : Number.parseInt(equipmentId, 10);
        await uploadLibraryDocument({
          category,
          equipmentId: eq != null && !Number.isNaN(eq) ? eq : null,
          title: title.trim(),
          fileName: file.name,
          fileBytes: bytes,
          mimeType: file.type || "application/octet-stream",
          notes: notes.trim() ? notes.trim() : null,
        });
        setTitle("");
        setNotes("");
        setEquipmentId("__none__");
        setFileInputKey((k) => k + 1);
        await load();
      } catch (e) {
        setError(toErrorMessage(e));
      } finally {
        setUploading(false);
      }
    },
    [category, equipmentId, title, notes, t, load],
  );

  const handleDownload = useCallback(async (doc: LibraryDocument) => {
    try {
      const bytes = await getLibraryDocumentFile(doc.id);
      const copy = new Uint8Array(bytes.byteLength);
      copy.set(bytes);
      const blob = new Blob([copy], { type: doc.mimeType || "application/octet-stream" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = doc.fileName;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(toErrorMessage(e));
    }
  }, []);

  const handleDelete = useCallback(
    async (id: number) => {
      if (!window.confirm(t("confirm.delete"))) return;
      try {
        await deleteLibraryDocument(id);
        await load();
      } catch (e) {
        setError(toErrorMessage(e));
      }
    },
    [t, load],
  );

  const openEdit = useCallback((doc: LibraryDocument) => {
    setEditing(doc);
    setEditTitle(doc.title);
    setEditEquipmentId(doc.equipmentId != null ? String(doc.equipmentId) : "__none__");
    setEditOpen(true);
  }, []);

  const saveEdit = useCallback(async () => {
    if (!editing) return;
    setSavingEdit(true);
    try {
      await updateLibraryDocument({
        id: editing.id,
        title: editTitle.trim() || editing.title,
        clearEquipmentLink: editEquipmentId === "__none__",
        equipmentId: editEquipmentId === "__none__" ? null : Number.parseInt(editEquipmentId, 10),
      });
      setEditOpen(false);
      setEditing(null);
      await load();
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setSavingEdit(false);
    }
  }, [editing, editTitle, editEquipmentId, load]);

  if (!category || !categorySlug || !isDocumentationCategorySlug(categorySlug)) {
    return <Navigate to="/documentation/technical-manuals" replace />;
  }

  return (
    <div className={cn(mfLayout.moduleWorkspace, "p-6")}>
      <div className="mb-6 max-w-3xl">
        <h2 className={mfLayout.moduleTitle}>{t(`categories.${categorySlug}.heading`)}</h2>
        <p className="mt-1 text-sm text-text-muted">
          {t(`categories.${categorySlug}.description`)}
        </p>
      </div>

      {error ? (
        <div className="mb-4 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      ) : null}

      <PermissionGate permission="doc.manage">
        <div className="mb-8 max-w-xl space-y-3 rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-medium text-text-primary">{t("upload.title")}</h3>
          <div className="grid gap-2">
            <Label htmlFor="doc-title">{t("upload.fields.title")}</Label>
            <Input
              id="doc-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t("upload.fields.titlePlaceholder")}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("upload.fields.equipment")}</Label>
            <Select value={equipmentId} onValueChange={setEquipmentId}>
              <SelectTrigger>
                <SelectValue placeholder={t("upload.fields.equipmentPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">{t("upload.fields.noEquipment")}</SelectItem>
                {assets.map((a) => (
                  <SelectItem key={a.id} value={String(a.id)}>
                    {a.asset_code} — {a.asset_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="doc-notes">{t("upload.fields.notes")}</Label>
            <Input
              id="doc-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t("upload.fields.notesPlaceholder")}
            />
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <input
              key={fileInputKey}
              type="file"
              className="text-sm text-text-secondary file:me-2 file:rounded file:border file:border-surface-border file:bg-surface-2 file:px-2 file:py-1"
              disabled={!canManage || uploading}
              onChange={(e) => void handleUpload(e.target.files)}
            />
            {uploading ? (
              <span className="inline-flex items-center gap-1 text-xs text-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("upload.uploading")}
              </span>
            ) : null}
          </div>
          <p className="text-xs text-text-muted">{t("upload.maxSize", { mb: 25 })}</p>
        </div>
      </PermissionGate>

      <div className="overflow-hidden rounded-lg border border-surface-border">
        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
          </div>
        ) : docs.length === 0 ? (
          <p className="py-10 text-center text-sm text-text-muted">{t("list.empty")}</p>
        ) : (
          <table className="w-full text-left text-sm">
            <thead className="border-b border-surface-border bg-surface-1 text-xs uppercase text-text-muted">
              <tr>
                <th className="px-3 py-2">{t("list.columns.title")}</th>
                <th className="px-3 py-2">{t("list.columns.file")}</th>
                <th className="px-3 py-2">{t("list.columns.equipment")}</th>
                <th className="px-3 py-2">{t("list.columns.uploaded")}</th>
                <th className="px-3 py-2 text-end">{t("list.columns.actions")}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-surface-border">
              {docs.map((doc) => (
                <tr key={doc.id} className="hover:bg-surface-1/60">
                  <td className="px-3 py-2 font-medium text-text-primary">{doc.title}</td>
                  <td className="px-3 py-2 text-text-secondary">{doc.fileName}</td>
                  <td className="px-3 py-2 text-text-secondary">
                    {doc.equipmentCode ? (
                      <span className="font-mono text-xs">{doc.equipmentCode}</span>
                    ) : (
                      <span className="text-text-muted">—</span>
                    )}
                  </td>
                  <td className="px-3 py-2 text-xs text-text-muted">
                    {new Date(doc.uploadedAt).toLocaleString(i18n.language)}
                  </td>
                  <td className="px-3 py-2">
                    <div className="flex justify-end gap-1">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title={t("list.download")}
                        onClick={() => void handleDownload(doc)}
                      >
                        <Download className="h-4 w-4" />
                      </Button>
                      {canManage ? (
                        <>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            title={t("list.edit")}
                            onClick={() => openEdit(doc)}
                          >
                            <Pencil className="h-4 w-4" />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            title={t("list.delete")}
                            onClick={() => void handleDelete(doc.id)}
                          >
                            <Trash2 className="h-4 w-4 text-destructive" />
                          </Button>
                        </>
                      ) : null}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("edit.title")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3 py-2">
            <div className="grid gap-2">
              <Label>{t("upload.fields.title")}</Label>
              <Input value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
            </div>
            <div className="grid gap-2">
              <Label>{t("upload.fields.equipment")}</Label>
              <Select value={editEquipmentId} onValueChange={setEditEquipmentId}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("upload.fields.noEquipment")}</SelectItem>
                  {assets.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {a.asset_code} — {a.asset_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setEditOpen(false)}>
              {t("edit.cancel")}
            </Button>
            <Button type="button" onClick={() => void saveEdit()} disabled={savingEdit}>
              {savingEdit ? <Loader2 className="h-4 w-4 animate-spin" /> : t("edit.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

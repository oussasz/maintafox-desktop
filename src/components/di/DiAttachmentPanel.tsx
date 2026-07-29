/**
 * DiAttachmentPanel.tsx
 *
 * Attachment list + upload drop zone for a DI.
 * Photos show inline previews (same approach as equipment gallery).
 */

import {
  FileUp,
  Trash2,
  File as FileIcon,
  Image,
  FileText,
  Activity,
  Loader2,
  AlertCircle,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
  listDiAttachments,
  uploadDiAttachment,
  uploadDiAttachmentFromPath,
  deleteDiAttachment,
  readDiAttachmentPreview,
  fileToNumberArray,
  MAX_ATTACHMENT_SIZE_BYTES,
} from "@/services/di-attachment-service";
import { toErrorMessage } from "@/utils/errors";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { DiAttachment, DiAttachmentType } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiAttachmentPanelProps {
  diId: number;
  canUpload: boolean;
  canDelete: boolean;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const MAX_SIZE_MB = MAX_ATTACHMENT_SIZE_BYTES / (1024 * 1024);

function inferAttachmentType(mime: string): DiAttachmentType {
  if (mime.startsWith("image/")) return "photo";
  if (mime === "application/pdf") return "pdf";
  return "other";
}

function attachmentIcon(type: string) {
  switch (type) {
    case "photo":
      return <Image className="h-5 w-5 text-blue-500" />;
    case "pdf":
      return <FileText className="h-5 w-5 text-red-500" />;
    case "sensor_snapshot":
      return <Activity className="h-5 w-5 text-green-500" />;
    default:
      return <FileIcon className="h-5 w-5 text-muted-foreground" />;
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} Ko`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} Mo`;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiAttachmentPanel({ diId, canUpload, canDelete }: DiAttachmentPanelProps) {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [attachments, setAttachments] = useState<DiAttachment[]>([]);
  const [previews, setPreviews] = useState<Record<number, string>>({});
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);
  const [lightbox, setLightbox] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const loadAttachments = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listDiAttachments(diId);
      setAttachments(items);

      const nextPreviews: Record<number, string> = {};
      await Promise.all(
        items
          .filter((a) => a.attachment_type === "photo" || a.mime_type.startsWith("image/"))
          .map(async (a) => {
            try {
              const preview = await readDiAttachmentPreview(a.id);
              nextPreviews[a.id] = `data:${preview.mime_type};base64,${preview.data_base64}`;
            } catch {
              // Preview is best-effort; list still works without it.
            }
          }),
      );
      setPreviews(nextPreviews);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [diId]);

  useEffect(() => {
    void loadAttachments();
  }, [loadAttachments]);

  const handleFiles = useCallback(
    async (files: FileList | File[]) => {
      const fileArray = Array.from(files);
      if (fileArray.length === 0) return;

      for (const file of fileArray) {
        if (file.size > MAX_ATTACHMENT_SIZE_BYTES) {
          setError(t("attachments.fileTooBig", { name: file.name, mb: MAX_SIZE_MB }));
          return;
        }
      }

      setError(null);
      setUploading(true);

      try {
        for (const file of fileArray) {
          const withPath = file as File & { path?: string };
          if (typeof withPath.path === "string" && withPath.path.length > 0) {
            await uploadDiAttachmentFromPath({
              diId,
              sourcePath: withPath.path,
              attachmentType: inferAttachmentType(file.type || ""),
            });
          } else {
            const bytes = await fileToNumberArray(file);
            await uploadDiAttachment({
              diId,
              fileName: file.name,
              fileBytes: bytes,
              mimeType: file.type || "application/octet-stream",
              attachmentType: inferAttachmentType(file.type),
            });
          }
        }
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setUploading(false);
      }
    },
    [diId, loadAttachments, t],
  );

  const handlePickNative = useCallback(async () => {
    if (!canUpload || uploading) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: "Files",
            extensions: [
              "png",
              "jpg",
              "jpeg",
              "webp",
              "gif",
              "pdf",
              "doc",
              "docx",
              "xls",
              "xlsx",
              "txt",
              "csv",
              "zip",
            ],
          },
        ],
      });
      if (!selected) return;
      const paths = Array.isArray(selected)
        ? selected.map((s) => (typeof s === "string" ? s : (s as { path?: string }).path)).filter(Boolean)
        : [typeof selected === "string" ? selected : (selected as { path?: string }).path].filter(
            Boolean,
          );

      setUploading(true);
      setError(null);
      try {
        for (const path of paths as string[]) {
          const lower = path.toLowerCase();
          const isImage = /\.(png|jpe?g|webp|gif)$/i.test(lower);
          await uploadDiAttachmentFromPath({
            diId,
            sourcePath: path,
            attachmentType: isImage ? "photo" : null,
          });
        }
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setUploading(false);
      }
    } catch (err) {
      // Fall back to HTML file input when dialog plugin is unavailable.
      fileInputRef.current?.click();
      if (err) {
        // keep silent — input click is the fallback
      }
    }
  }, [canUpload, uploading, diId, loadAttachments]);

  const handleDelete = useCallback(
    async (attachmentId: number) => {
      setConfirmDeleteId(null);
      try {
        setError(null);
        await deleteDiAttachment(attachmentId);
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      }
    },
    [loadAttachments],
  );

  const onDragOver = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      if (canUpload) setDragOver(true);
    },
    [canUpload],
  );

  const onDragLeave = useCallback(() => setDragOver(false), []);

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragOver(false);
      if (!canUpload || uploading) return;

      const paths = Array.from(e.dataTransfer.files ?? [])
        .map((f) => {
          const withPath = f as File & { path?: string };
          return typeof withPath.path === "string" ? withPath.path : "";
        })
        .filter(Boolean);

      if (paths.length > 0) {
        void (async () => {
          setUploading(true);
          setError(null);
          try {
            for (const path of paths) {
              const isImage = /\.(png|jpe?g|webp|gif)$/i.test(path);
              await uploadDiAttachmentFromPath({
                diId,
                sourcePath: path,
                attachmentType: isImage ? "photo" : null,
              });
            }
            await loadAttachments();
          } catch (err) {
            setError(toErrorMessage(err));
          } finally {
            setUploading(false);
          }
        })();
        return;
      }

      const files = e.dataTransfer.files;
      if (files.length > 0) void handleFiles(files);
      else void handlePickNative();
    },
    [canUpload, uploading, handleFiles, handlePickNative, diId, loadAttachments],
  );

  const onFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (files && files.length > 0) void handleFiles(files);
      e.target.value = "";
    },
    [handleFiles],
  );

  return (
    <div className="space-y-4">
      <h3 className="text-sm font-semibold text-text-primary">{t("attachments.heading")}</h3>

      {error && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {canUpload && (
        <div
          role="button"
          tabIndex={0}
          className={cn(
            "flex cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed p-6 text-center transition-colors",
            dragOver
              ? "border-primary bg-primary/5"
              : "border-surface-border hover:border-primary/50",
            uploading && "pointer-events-none opacity-60",
          )}
          onDragOver={onDragOver}
          onDragLeave={onDragLeave}
          onDrop={onDrop}
          onClick={() => {
            if (!uploading) void handlePickNative();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              if (!uploading) void handlePickNative();
            }
          }}
        >
          {uploading ? (
            <Loader2 className="h-6 w-6 animate-spin text-primary" />
          ) : (
            <FileUp className="h-6 w-6 text-muted-foreground" />
          )}
          <p className="text-sm text-muted-foreground">
            {uploading ? t("attachments.uploading") : t("attachments.dropHint")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("attachments.maxSize", { mb: MAX_SIZE_MB })}
          </p>
          <input
            ref={fileInputRef}
            type="file"
            className="hidden"
            multiple
            onChange={onFileInputChange}
          />
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      ) : attachments.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">{t("attachments.empty")}</p>
      ) : (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {attachments.map((att) => {
            const previewUrl = previews[att.id];
            return (
              <li key={att.id} className="flex items-center gap-3 px-3 py-2.5">
                {previewUrl ? (
                  <button
                    type="button"
                    className="h-12 w-12 shrink-0 overflow-hidden rounded border border-surface-border"
                    onClick={() => setLightbox(previewUrl)}
                    title={att.file_name}
                  >
                    <img
                      src={previewUrl}
                      alt={att.file_name}
                      className="h-full w-full object-cover"
                    />
                  </button>
                ) : (
                  attachmentIcon(att.attachment_type)
                )}

                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-text-primary">{att.file_name}</p>
                  <p className="text-xs text-muted-foreground">
                    {formatBytes(att.size_bytes)} ·{" "}
                    {new Date(att.uploaded_at).toLocaleDateString(dateLocale, {
                      day: "2-digit",
                      month: "short",
                      year: "numeric",
                    })}
                  </p>
                </div>

                {canDelete && (
                  <>
                    {confirmDeleteId === att.id ? (
                      <div className="flex items-center gap-1">
                        <Button
                          variant="destructive"
                          size="sm"
                          onClick={() => handleDelete(att.id)}
                        >
                          {t("attachments.confirmDelete")}
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setConfirmDeleteId(null)}>
                          {t("attachments.cancelDelete")}
                        </Button>
                      </div>
                    ) : (
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => setConfirmDeleteId(att.id)}
                        title={t("attachments.deleteTitle")}
                      >
                        <Trash2 className="h-4 w-4 text-muted-foreground" />
                      </Button>
                    )}
                  </>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {lightbox && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
          role="dialog"
          aria-modal="true"
          onClick={() => setLightbox(null)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setLightbox(null);
          }}
        >
          <img
            src={lightbox}
            alt=""
            className="max-h-[90vh] max-w-[90vw] rounded shadow-lg"
            onClick={(e) => e.stopPropagation()}
          />
        </div>
      )}
    </div>
  );
}

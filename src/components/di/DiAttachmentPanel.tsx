/**
 * DiAttachmentPanel.tsx
 *
 * Attachment list + upload drop zone for a DI.
 * Phase 2 – Sub-phase 04 – File 03 – Sprint S3.
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

import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import {
  listDiAttachments,
  uploadDiAttachment,
  deleteDiAttachment,
  fileToNumberArray,
  MAX_ATTACHMENT_SIZE_BYTES,
} from "@/services/di-attachment-service";
import { toErrorMessage } from "@/utils/errors";
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
  const [attachments, setAttachments] = useState<DiAttachment[]>([]);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // ── Load attachments ────────────────────────────────────────────────────

  const loadAttachments = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listDiAttachments(diId);
      setAttachments(items);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [diId]);

  useEffect(() => {
    void loadAttachments();
  }, [loadAttachments]);

  // ── Upload handler ──────────────────────────────────────────────────────

  const handleFiles = useCallback(
    async (files: FileList | File[]) => {
      const fileArray = Array.from(files);
      if (fileArray.length === 0) return;

      // Validate size before any IPC call
      for (const file of fileArray) {
        if (file.size > MAX_ATTACHMENT_SIZE_BYTES) {
          setError(`Le fichier « ${file.name} » dépasse la taille maximale de ${MAX_SIZE_MB} Mo.`);
          return;
        }
      }

      setError(null);
      setUploading(true);

      try {
        for (const file of fileArray) {
          const bytes = await fileToNumberArray(file);
          await uploadDiAttachment({
            diId,
            fileName: file.name,
            fileBytes: bytes,
            mimeType: file.type || "application/octet-stream",
            attachmentType: inferAttachmentType(file.type),
          });
        }
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setUploading(false);
      }
    },
    [diId, loadAttachments],
  );

  // ── Delete handler ──────────────────────────────────────────────────────

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

  // ── Drag & drop handlers ───────────────────────────────────────────────

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
      const files = e.dataTransfer.files;
      if (files.length > 0) void handleFiles(files);
    },
    [canUpload, uploading, handleFiles],
  );

  const onFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (files && files.length > 0) void handleFiles(files);
      // Reset input so re-selecting the same file triggers change
      e.target.value = "";
    },
    [handleFiles],
  );

  // ── Render ──────────────────────────────────────────────────────────────

  return (
    <div className="space-y-4">
      <h3 className="text-sm font-semibold text-text-primary">Pièces jointes</h3>

      {/* Error banner */}
      {error && (
        <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {/* Drop zone */}
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
          onClick={() => !uploading && fileInputRef.current?.click()}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              if (!uploading) fileInputRef.current?.click();
            }
          }}
        >
          {uploading ? (
            <Loader2 className="h-6 w-6 animate-spin text-primary" />
          ) : (
            <FileUp className="h-6 w-6 text-muted-foreground" />
          )}
          <p className="text-sm text-muted-foreground">
            {uploading
              ? "Téléversement en cours…"
              : "Glisser-déposer ou cliquer pour ajouter un fichier"}
          </p>
          <p className="text-xs text-muted-foreground">Max {MAX_SIZE_MB} Mo</p>
          <input
            ref={fileInputRef}
            type="file"
            className="hidden"
            multiple
            onChange={onFileInputChange}
          />
        </div>
      )}

      {/* Attachment list */}
      {loading ? (
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      ) : attachments.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">Aucune pièce jointe.</p>
      ) : (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {attachments.map((att) => (
            <li key={att.id} className="flex items-center gap-3 px-3 py-2.5">
              {attachmentIcon(att.attachment_type)}

              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-text-primary">{att.file_name}</p>
                <p className="text-xs text-muted-foreground">
                  {formatBytes(att.size_bytes)} ·{" "}
                  {new Date(att.uploaded_at).toLocaleDateString("fr-FR", {
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
                      <Button variant="destructive" size="sm" onClick={() => handleDelete(att.id)}>
                        Confirmer
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => setConfirmDeleteId(null)}>
                        Annuler
                      </Button>
                    </div>
                  ) : (
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => setConfirmDeleteId(att.id)}
                      title="Supprimer la pièce jointe"
                    >
                      <Trash2 className="h-4 w-4 text-muted-foreground" />
                    </Button>
                  )}
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * WoAttachmentPanel.tsx
 *
 * Attachment list + upload drop zone for a Work Order.
 * Phase column (before|during|after|evidence) required on upload;
 * list is grouped by phase.
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
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import {
  listWoAttachments,
  uploadWoAttachment,
  deleteWoAttachment,
  fileToNumberArray,
  MAX_WO_ATTACHMENT_SIZE_BYTES,
  type WoAttachment,
  type WoAttachmentPhase,
} from "@/services/wo-closeout-service";
import { toErrorMessage } from "@/utils/errors";

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoAttachmentPanelProps {
  woId: number;
  canUpload: boolean;
  canDelete: boolean;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const MAX_SIZE_MB = MAX_WO_ATTACHMENT_SIZE_BYTES / (1024 * 1024);

const PHASE_OPTIONS: WoAttachmentPhase[] = ["before", "during", "after", "evidence"];

function inferMimeCategory(mime: string): "photo" | "pdf" | "other" {
  if (mime.startsWith("image/")) return "photo";
  if (mime === "application/pdf") return "pdf";
  return "other";
}

function attachmentIcon(mime: string) {
  const category = inferMimeCategory(mime);
  switch (category) {
    case "photo":
      return <Image className="h-5 w-5 text-blue-500" />;
    case "pdf":
      return <FileText className="h-5 w-5 text-red-500" />;
    default:
      if (mime.startsWith("text/")) {
        return <Activity className="h-5 w-5 text-green-500" />;
      }
      return <FileIcon className="h-5 w-5 text-muted-foreground" />;
  }
}

function formatBytes(bytes: number, t: (key: string) => string): string {
  if (bytes < 1024) return `${bytes} ${t("attachment.bytes")}`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} ${t("attachment.kilobytes")}`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} ${t("attachment.megabytes")}`;
}

function groupByPhase(attachments: WoAttachment[]): Map<string, WoAttachment[]> {
  const order: Array<WoAttachmentPhase | "unset"> = [
    "before",
    "during",
    "after",
    "evidence",
    "unset",
  ];
  const map = new Map<string, WoAttachment[]>();
  for (const phase of order) map.set(phase, []);
  for (const att of attachments) {
    const key = att.phase ?? "unset";
    if (!map.has(key)) map.set(key, []);
    const group = map.get(key);
    if (group) group.push(att);
  }
  // Remove empty groups
  for (const [key, list] of map) {
    if (list.length === 0) map.delete(key);
  }
  return map;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoAttachmentPanel({ woId, canUpload, canDelete }: WoAttachmentPanelProps) {
  const { t, i18n } = useTranslation("ot");
  const [attachments, setAttachments] = useState<WoAttachment[]>([]);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Phase selection dialog (shown before uploading dropped/selected files)
  const [pendingFiles, setPendingFiles] = useState<File[] | null>(null);
  const [selectedPhase, setSelectedPhase] = useState<WoAttachmentPhase>("during");

  // ── Load attachments ────────────────────────────────────────────────

  const loadAttachments = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listWoAttachments(woId);
      setAttachments(items);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [woId]);

  useEffect(() => {
    void loadAttachments();
  }, [loadAttachments]);

  // ── Upload handler ──────────────────────────────────────────────────

  const doUpload = useCallback(
    async (files: File[], phase: WoAttachmentPhase) => {
      if (files.length === 0) return;

      for (const file of files) {
        if (file.size > MAX_WO_ATTACHMENT_SIZE_BYTES) {
          setError(t("attachment.fileTooLarge", { name: file.name, size: MAX_SIZE_MB }));
          return;
        }
      }

      setError(null);
      setUploading(true);

      try {
        for (const file of files) {
          const bytes = await fileToNumberArray(file);
          await uploadWoAttachment({
            woId,
            fileName: file.name,
            fileBytes: bytes,
            mimeType: file.type || "application/octet-stream",
            phase,
          });
        }
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setUploading(false);
        setPendingFiles(null);
      }
    },
    [woId, loadAttachments, t],
  );

  const handleFiles = useCallback(
    (files: FileList | File[]) => {
      const fileArray = Array.from(files);
      if (fileArray.length === 0) return;
      // Validate size early
      for (const file of fileArray) {
        if (file.size > MAX_WO_ATTACHMENT_SIZE_BYTES) {
          setError(t("attachment.fileTooLarge", { name: file.name, size: MAX_SIZE_MB }));
          return;
        }
      }
      setSelectedPhase("during");
      setPendingFiles(fileArray);
    },
    [t],
  );

  // ── Delete handler ──────────────────────────────────────────────────

  const handleDelete = useCallback(
    async (attachmentId: number) => {
      setConfirmDeleteId(null);
      try {
        setError(null);
        await deleteWoAttachment(attachmentId);
        await loadAttachments();
      } catch (err) {
        setError(toErrorMessage(err));
      }
    },
    [loadAttachments],
  );

  // ── Drag & drop handlers ────────────────────────────────────────────

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
      if (files.length > 0) handleFiles(files);
    },
    [canUpload, uploading, handleFiles],
  );

  const onFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (files && files.length > 0) handleFiles(files);
      e.target.value = "";
    },
    [handleFiles],
  );

  // ── Grouped display ────────────────────────────────────────────────

  const grouped = groupByPhase(attachments);

  // ── Render ──────────────────────────────────────────────────────────

  return (
    <div className="space-y-4">
      <h3 className="text-sm font-semibold text-text-primary">{t("attachment.title")}</h3>

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
            {uploading ? t("attachment.uploading") : t("attachment.dropHint")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("attachment.maxSize", { size: MAX_SIZE_MB })}
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

      {/* Attachment list grouped by phase */}
      {loading ? (
        <div className="flex items-center justify-center py-6">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      ) : attachments.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">{t("attachment.empty")}</p>
      ) : (
        <div className="space-y-4">
          {Array.from(grouped.entries()).map(([phase, list]) => (
            <div key={phase}>
              <p className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                {t(`attachment.phase.${phase}`, { defaultValue: phase })}
              </p>
              <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
                {list.map((att) => (
                  <li key={att.id} className="flex items-center gap-3 px-3 py-2.5">
                    {attachmentIcon(att.mime_type)}

                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium text-text-primary">
                        {att.file_name}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {formatBytes(att.size_bytes, t)} ·{" "}
                        {new Date(att.uploaded_at).toLocaleDateString(i18n.language, {
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
                              onClick={() => void handleDelete(att.id)}
                            >
                              {t("attachment.confirm")}
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              onClick={() => setConfirmDeleteId(null)}
                            >
                              {t("attachment.cancel")}
                            </Button>
                          </div>
                        ) : (
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={() => setConfirmDeleteId(att.id)}
                            title={t("attachment.deleteTitle")}
                          >
                            <Trash2 className="h-4 w-4 text-muted-foreground" />
                          </Button>
                        )}
                      </>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}

      {/* Phase selection dialog before upload */}
      <Dialog open={pendingFiles != null} onOpenChange={(open) => !open && setPendingFiles(null)}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("attachment.phaseDialog.title")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 py-2">
            <p className="text-sm text-muted-foreground">
              {t("attachment.phaseDialog.hint", {
                count: pendingFiles?.length ?? 0,
              })}
            </p>
            <div className="space-y-1">
              <Label>{t("attachment.phaseDialog.phaseLabel")}</Label>
              <Select
                value={selectedPhase}
                onValueChange={(v) => setSelectedPhase(v as WoAttachmentPhase)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PHASE_OPTIONS.map((p) => (
                    <SelectItem key={p} value={p}>
                      {t(`attachment.phase.${p}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingFiles(null)}>
              {t("attachment.cancel")}
            </Button>
            <Button
              onClick={() => void doUpload(pendingFiles ?? [], selectedPhase)}
              disabled={uploading}
            >
              {uploading ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {t("attachment.phaseDialog.upload")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

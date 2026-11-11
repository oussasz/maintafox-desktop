/**
 * Versions of a library document (L7-17): upload a new version (new file + revision label) and
 * read the history (every earlier version stays downloadable, read-only).
 */
import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useFormatters } from "@/hooks/use-formatters";
import {
  MAX_LIBRARY_DOCUMENT_BYTES,
  fileToNumberArray,
  listLibraryDocumentVersions,
  uploadLibraryDocumentVersion,
} from "@/services/documentation-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { LibraryDocument } from "@shared/ipc-types";

const MAX_MB = Math.floor(MAX_LIBRARY_DOCUMENT_BYTES / (1024 * 1024));

// ── New version ───────────────────────────────────────────────────────────────

interface NewVersionDialogProps {
  /** The current version being replaced; null when closed. */
  doc: LibraryDocument | null;
  onOpenChange: (open: boolean) => void;
  onUploaded: () => void;
}

export function DocumentNewVersionDialog({ doc, onOpenChange, onUploaded }: NewVersionDialogProps) {
  const { t } = useTranslation("documentation");
  const [file, setFile] = useState<File | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [revision, setRevision] = useState("");
  const [validUntil, setValidUntil] = useState("");
  const [validUntilInvalid, setValidUntilInvalid] = useState(false);
  const [notes, setNotes] = useState("");
  const [uploading, setUploading] = useState(false);
  const [fileInputKey, setFileInputKey] = useState(0);

  useEffect(() => {
    if (!doc) return;
    setFile(null);
    setFileError(null);
    setRevision("");
    // The current validity is proposed; a new revision usually brings its own date.
    setValidUntil(doc.validUntil ?? "");
    setValidUntilInvalid(false);
    setNotes("");
    setFileInputKey((k) => k + 1);
  }, [doc]);

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
  };

  const sameRevision =
    doc?.revision != null &&
    revision.trim() !== "" &&
    doc.revision.trim().toLowerCase() === revision.trim().toLowerCase();
  const canUpload =
    doc != null &&
    file != null &&
    revision.trim() !== "" &&
    !sameRevision &&
    !validUntilInvalid &&
    !uploading;

  const upload = async () => {
    if (!doc || !file || !canUpload) return;
    setUploading(true);
    try {
      await uploadLibraryDocumentVersion({
        id: doc.id,
        fileName: file.name,
        fileBytes: await fileToNumberArray(file),
        mimeType: file.type || "application/octet-stream",
        notes: notes.trim() ? notes.trim() : null,
        revision: revision.trim(),
        validUntil: validUntil.trim() ? validUntil : null,
      });
      pushAppToast({
        title: t("versions.done", { title: doc.title, revision: revision.trim() }),
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
    <Dialog open={doc != null} onOpenChange={(o) => !uploading && onOpenChange(o)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("versions.newTitle")}</DialogTitle>
          <DialogDescription>
            {doc
              ? t("versions.newDescription", {
                  title: doc.title,
                  revision: doc.revision ?? t("versions.noRevision"),
                })
              : ""}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-1">
          <div className="grid gap-1.5">
            <Label htmlFor="doc-version-file">
              {t("upload.fields.file")} <span aria-hidden>*</span>
            </Label>
            <input
              key={fileInputKey}
              id="doc-version-file"
              type="file"
              aria-describedby={fileError ? "doc-version-file-error" : undefined}
              aria-invalid={fileError ? true : undefined}
              className="min-h-10 text-sm text-text-secondary file:me-3 file:h-10 file:cursor-pointer file:rounded-md file:border file:border-surface-border file:bg-surface-1 file:px-3 file:text-sm file:font-medium file:text-text-primary hover:file:bg-surface-2"
              disabled={uploading}
              onChange={(e) => pickFile(e.target.files)}
            />
            {fileError ? (
              <p id="doc-version-file-error" role="alert" className="text-xs text-tone-danger-fg">
                {fileError}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-version-revision">
              {t("upload.fields.revision")} <span aria-hidden>*</span>
            </Label>
            <Input
              id="doc-version-revision"
              value={revision}
              maxLength={64}
              required
              aria-invalid={sameRevision || undefined}
              aria-describedby={sameRevision ? "doc-version-revision-error" : undefined}
              onChange={(e) => setRevision(e.target.value)}
              placeholder={t("upload.fields.revisionPlaceholder")}
              disabled={uploading}
            />
            {sameRevision ? (
              <p
                id="doc-version-revision-error"
                role="alert"
                className="text-xs text-tone-danger-fg"
              >
                {t("versions.sameRevision")}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-version-valid-until">{t("upload.fields.validUntil")}</Label>
            <DateField
              id="doc-version-valid-until"
              value={validUntil}
              onChange={setValidUntil}
              onInvalidChange={setValidUntilInvalid}
              disabled={uploading}
              aria-invalid={validUntilInvalid || undefined}
            />
            {validUntilInvalid ? (
              <p role="alert" className="text-xs text-tone-danger-fg">
                {t("upload.fields.validUntilInvalid")}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="doc-version-notes">{t("versions.changes")}</Label>
            <Input
              id="doc-version-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t("versions.changesPlaceholder")}
              disabled={uploading}
            />
          </div>
          <p className="text-xs text-text-muted">{t("versions.keepsHistory")}</p>
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
            {uploading ? t("upload.uploading") : t("versions.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── History ───────────────────────────────────────────────────────────────────

interface HistoryDialogProps {
  /** The current version whose history is shown; null when closed. */
  doc: LibraryDocument | null;
  onOpenChange: (open: boolean) => void;
  onDownload: (version: LibraryDocument) => void;
}

export function DocumentHistoryDialog({ doc, onOpenChange, onDownload }: HistoryDialogProps) {
  const { t } = useTranslation("documentation");
  const { formatDate, formatDateTime } = useFormatters();
  const [versions, setVersions] = useState<LibraryDocument[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!doc) return;
    let cancelled = false;
    setVersions(null);
    setError(null);
    listLibraryDocumentVersions(doc.id)
      .then((rows) => {
        if (!cancelled) setVersions(rows);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [doc, attempt]);

  return (
    <Dialog open={doc != null} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("versions.historyTitle", { title: doc?.title ?? "" })}</DialogTitle>
          <DialogDescription>{t("versions.historyDescription")}</DialogDescription>
        </DialogHeader>
        {error != null ? (
          <ErrorState
            error={error}
            title={t("versions.loadError")}
            onRetry={() => setAttempt((a) => a + 1)}
          />
        ) : versions == null ? (
          <LoadingState />
        ) : (
          <ol className="grid gap-2">
            {versions.map((v, i) => (
              <li
                key={v.id}
                className="flex items-start justify-between gap-3 rounded-md border border-surface-border px-3 py-2"
              >
                <div className="min-w-0 text-sm">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium text-text-primary">
                      {v.revision
                        ? t("list.revisionLabel", { revision: v.revision })
                        : t("versions.noRevision")}
                    </span>
                    {i === 0 && v.supersededAt == null ? (
                      <Badge variant="secondary">{t("versions.current")}</Badge>
                    ) : null}
                  </div>
                  <div className="text-xs text-text-secondary">
                    {t("versions.uploaded", {
                      date: formatDateTime(v.uploadedAt),
                      by: v.uploadedByName ?? t("versions.unknownUser"),
                    })}
                    {v.supersededAt
                      ? ` · ${t("versions.replaced", { date: formatDateTime(v.supersededAt) })}`
                      : null}
                  </div>
                  <div className="break-all text-xs text-text-muted">
                    {v.fileName}
                    {v.validUntil
                      ? ` · ${t("list.validity.until", { date: formatDate(v.validUntil) })}`
                      : null}
                  </div>
                  {v.notes ? <div className="text-xs text-text-secondary">{v.notes}</div> : null}
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  className="h-10 w-10 shrink-0 p-0"
                  aria-label={t("versions.downloadNamed", {
                    revision: v.revision ?? t("versions.noRevision"),
                  })}
                  title={t("list.download")}
                  onClick={() => onDownload(v)}
                >
                  <Download className="h-4 w-4" aria-hidden />
                </Button>
              </li>
            ))}
          </ol>
        )}
        <DialogFooter>
          <Button variant="outline" className="h-11" onClick={() => onOpenChange(false)}>
            {t("versions.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

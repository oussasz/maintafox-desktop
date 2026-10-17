/**
 * SupportBundleDialog.tsx
 *
 * Shows application info, lets the user copy the sanitized diagnostic bundle, and sends it to
 * Maintafox support (upload_support_bundle; with a note the server opens a support ticket).
 *
 * Used from: Settings → Support & Diagnostics (Phase 2 UI module)
 * For Phase 1 it is accessible from the developer debug menu only.
 */

import { ClipboardCopy, Download, Loader2, Send, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  getDiagnosticsInfo,
  generateSupportBundle,
  uploadSupportBundle,
  type SupportBundleUploadResult,
} from "@/services/diagnostics-service";
import type { DiagnosticsAppInfo, SupportBundle } from "@shared/ipc-types";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SupportBundleDialog({ open, onClose }: Props) {
  const { t } = useTranslation("diagnostics");
  const [appInfo, setAppInfo] = useState<DiagnosticsAppInfo | null>(null);
  const [bundle, setBundle] = useState<SupportBundle | null>(null);
  const [isGenerating, setIsGenerating] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [sent, setSent] = useState<SupportBundleUploadResult | null>(null);

  const handleSend = useCallback(async () => {
    setIsSending(true);
    setError(null);
    setSent(null);
    try {
      const trimmed = note.trim();
      setSent(await uploadSupportBundle(trimmed.length > 0 ? trimmed : null));
      setNote("");
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setIsSending(false);
    }
  }, [note]);

  const handleGenerate = useCallback(async () => {
    setIsGenerating(true);
    setError(null);
    try {
      const info = await getDiagnosticsInfo();
      const b = await generateSupportBundle();
      setAppInfo(info);
      setBundle(b);
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setIsGenerating(false);
    }
  }, []);

  const handleCopy = useCallback(async () => {
    if (!bundle) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(bundle, null, 2));
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 3000);
    } catch {
      setError(t("copyFailed"));
    }
  }, [bundle, t]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("dialogTitle")}
        className="w-full max-w-lg rounded-xl bg-surface-1 border border-surface-border shadow-xl"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-surface-border px-5 py-4">
          <h2 className="text-lg font-semibold text-text-primary">{t("dialogTitle")}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("close")}
            className="rounded-lg p-1 text-text-muted hover:bg-surface-2 hover:text-text-primary"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Body */}
        <div className="space-y-4 px-5 py-4">
          <p className="text-sm text-text-secondary">{t("description")}</p>

          {appInfo && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt className="text-text-muted">{t("version")}</dt>
              <dd className="font-mono text-text-primary">{appInfo.app_version}</dd>
              <dt className="text-text-muted">{t("os")}</dt>
              <dd className="text-text-primary">
                {appInfo.os_name} {appInfo.os_version} ({appInfo.arch})
              </dd>
              <dt className="text-text-muted">{t("dbSchema")}</dt>
              <dd className="text-text-primary">
                {t("migrationCount", { count: appInfo.db_schema_version })}
              </dd>
              <dt className="text-text-muted">{t("locale")}</dt>
              <dd className="text-text-primary">{appInfo.active_locale}</dd>
              <dt className="text-text-muted">{t("uptime")}</dt>
              <dd className="text-text-primary">
                {Math.floor(appInfo.uptime_seconds / 60)} {t("minutes")}
              </dd>
            </dl>
          )}

          {bundle && bundle.collection_warnings.length > 0 && (
            <ul
              role="list"
              className="space-y-1 rounded-lg bg-status-warning/10 border border-status-warning/30 p-3"
            >
              {bundle.collection_warnings.map((w, i) => (
                <li key={i} className="text-xs text-status-warning">
                  {w}
                </li>
              ))}
            </ul>
          )}

          <section className="space-y-2 rounded-lg border border-surface-border p-3">
            <h3 className="text-sm font-semibold text-text-primary">{t("sendTitle")}</h3>
            <p className="text-xs text-text-secondary">{t("sendHint")}</p>
            <textarea
              aria-label={t("notePlaceholder")}
              className="w-full rounded-md border border-surface-border bg-surface-1 p-2 text-sm"
              rows={3}
              maxLength={2000}
              value={note}
              onChange={(e) => setNote(e.target.value)}
              placeholder={t("notePlaceholder")}
            />
            <button
              type="button"
              onClick={() => void handleSend()}
              disabled={isSending}
              className="inline-flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary-dark disabled:opacity-50"
            >
              {isSending ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Send className="h-4 w-4" />
              )}
              {isSending ? t("sending") : t("send")}
            </button>
            {sent && (
              <p role="status" className="text-sm text-status-success">
                {sent.ticket_id
                  ? t("sentWithTicket", { bundle: sent.bundle_id, ticket: sent.ticket_id })
                  : t("sent", { bundle: sent.bundle_id })}
              </p>
            )}
          </section>

          {error && (
            <p
              role="alert"
              className="rounded-lg border border-tone-danger-border bg-tone-danger-bg p-3 text-sm text-tone-danger-fg"
            >
              {error}
            </p>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 border-t border-surface-border px-5 py-4">
          <button
            type="button"
            onClick={() => void handleGenerate()}
            disabled={isGenerating}
            className={cn(
              "inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium",
              "bg-primary text-white hover:bg-primary/90 disabled:opacity-50",
            )}
          >
            {isGenerating ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            {isGenerating ? t("generating") : t("generateBundle")}
          </button>

          <button
            type="button"
            onClick={() => void handleCopy()}
            disabled={!bundle || isGenerating}
            className={cn(
              "inline-flex items-center gap-2 rounded-lg border border-surface-border px-4 py-2 text-sm font-medium",
              "bg-surface-2 text-text-primary hover:bg-surface-3 disabled:opacity-50",
            )}
          >
            <ClipboardCopy className="h-4 w-4" />
            {copySuccess ? t("copied") : t("copyToClipboard")}
          </button>

          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-surface-border bg-surface-2 px-4 py-2 text-sm font-medium text-text-primary hover:bg-surface-3"
          >
            {t("close")}
          </button>
        </div>
      </div>
    </div>
  );
}

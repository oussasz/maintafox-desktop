/**
 * SupportBundleDialog.tsx
 *
 * Phase 1 stub. Shows application info and allows copying a diagnostic bundle
 * to the clipboard. Full ticketing integration is a Phase 2 feature.
 *
 * Used from: Settings → Support & Diagnostics (Phase 2 UI module)
 * For Phase 1 it is accessible from the developer debug menu only.
 */

import { ClipboardCopy, Download, Loader2, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { getDiagnosticsInfo, generateSupportBundle } from "@/services/diagnostics-service";
import { toErrorMessage } from "@/utils/errors";
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

  const handleGenerate = useCallback(async () => {
    setIsGenerating(true);
    setError(null);
    try {
      const info = await getDiagnosticsInfo();
      const b = await generateSupportBundle();
      setAppInfo(info);
      setBundle(b);
    } catch (err) {
      setError(toErrorMessage(err));
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

          {error && (
            <p
              role="alert"
              className="rounded-lg bg-status-error/10 border border-status-error/30 p-3 text-sm text-status-error"
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

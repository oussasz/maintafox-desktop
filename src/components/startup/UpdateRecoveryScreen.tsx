import { History } from "lucide-react";
import { useTranslation } from "react-i18next";

import { UpdateProgress } from "@/components/updates/UpdateProgress";
import { useLocaleStore } from "@/stores/locale-store";
import { useUpdaterStore } from "@/stores/updater-store";

interface UpdateRecoveryScreenProps {
  /** The version to reinstall (the one this device ran before the update). */
  fromVersion: string;
  /** The version that could not update the database (the one running now). */
  toVersion: string;
  /** Technical reason (migration error). */
  reason: string;
}

/**
 * Shown at startup when the version installed by an in-app update could not update the database
 * (master plan Phase 5.5). The backend has already staged the backup taken just before the update;
 * reinstalling the previous version puts it back. Nothing is changed until the user acts.
 */
export function UpdateRecoveryScreen({
  fromVersion,
  toVersion,
  reason,
}: UpdateRecoveryScreenProps) {
  const { t } = useTranslation("shell");
  const phase = useUpdaterStore((s) => s.phase);
  const downloadedBytes = useUpdaterStore((s) => s.downloadedBytes);
  const totalBytes = useUpdaterStore((s) => s.totalBytes);
  const refusal = useUpdaterStore((s) => s.refusal);
  const error = useUpdaterStore((s) => s.error);
  const allowedVersion = useUpdaterStore((s) =>
    s.refusal?.code === "rollback_refused" ? (s.journal?.rollback_allowed_version ?? null) : null,
  );
  const reinstallPrevious = useUpdaterStore((s) => s.reinstallPrevious);
  const locale = useLocaleStore((s) => s.activeLocale);

  const busy = phase !== "idle";
  // The server refused the rollback (refusal status, nothing offered, another version): keep the
  // PC as it is and contact the vendor; nothing else is tried. Other refusals: plain advice.
  const failureText = refusal
    ? refusal.code === "rollback_refused"
      ? t("updates.recovery.refused", { from: fromVersion })
      : t("updates.recovery.unavailable")
    : error
      ? t("updates.recovery.error", { from: fromVersion })
      : null;

  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 bg-surface-0 px-8 text-center">
      <History className="h-10 w-10 text-primary" aria-hidden />
      <p className="text-lg font-semibold">{t("updates.recovery.title")}</p>
      <p className="max-w-md text-sm text-text-secondary">
        {t("updates.recovery.body", { to: toVersion })}
      </p>
      <p className="max-w-md text-sm font-medium">
        {t("updates.recovery.action", { from: fromVersion })}
      </p>

      <div className="flex w-full max-w-md flex-col gap-3">
        {busy ? (
          <div className="text-left">
            <UpdateProgress
              phase={phase}
              downloadedBytes={downloadedBytes}
              totalBytes={totalBytes}
            />
          </div>
        ) : (
          <button
            type="button"
            className="btn-primary"
            onClick={() => void reinstallPrevious(locale, fromVersion)}
          >
            {t("updates.recovery.reinstall", { from: fromVersion })}
          </button>
        )}

        {failureText && !busy ? (
          <div role="alert" className="text-left text-sm text-text-danger">
            <p>{failureText}</p>
            {allowedVersion ? (
              // Information for support only: this version is never installed automatically.
              <p className="mt-1 text-text-secondary">
                {t("updates.recovery.allowedVersion", { version: allowedVersion })}
              </p>
            ) : null}
            {error ? (
              <p className="mt-1 text-xs text-text-muted">
                {t("updates.error.detail", { detail: error })}
              </p>
            ) : null}
          </div>
        ) : null}
      </div>

      {reason ? (
        <details className="max-w-md text-left text-xs text-text-muted">
          <summary className="cursor-pointer select-none">
            {t("updates.recovery.technical")}
          </summary>
          <p className="mt-1 whitespace-pre-wrap break-words font-mono">{reason}</p>
        </details>
      ) : null}
    </div>
  );
}

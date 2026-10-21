import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { UpdateInstallPhase } from "@/types";

const BYTES_PER_MEGABYTE = 1024 * 1024;
const PERCENT_MAX = 100;

interface UpdateProgressProps {
  /** Current phase — never "idle" (the caller shows this view only while installing). */
  phase: Exclude<UpdateInstallPhase, "idle">;
  downloadedBytes: number;
  totalBytes: number | null;
}

/**
 * Progress of an in-app install / reinstall, driven by the Rust progress channel:
 * downloading (real bytes, signature checked) → backing up or restoring the backup → installing /
 * restarting.
 * Shared by the update banner and the update recovery screen.
 */
export function UpdateProgress({ phase, downloadedBytes, totalBytes }: UpdateProgressProps) {
  const { t, i18n } = useTranslation("shell");
  const megabytes = new Intl.NumberFormat(i18n.language, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });

  const knownTotal = totalBytes !== null && totalBytes > 0 ? totalBytes : null;
  const percent =
    knownTotal !== null
      ? Math.min(PERCENT_MAX, Math.floor((downloadedBytes / knownTotal) * PERCENT_MAX))
      : null;
  const downloadedMb = megabytes.format(downloadedBytes / BYTES_PER_MEGABYTE);

  return (
    <div className="flex w-full flex-col gap-2" aria-live="polite">
      <p className="flex items-center gap-2 text-sm font-medium text-text-primary">
        <Loader2 className="h-4 w-4 shrink-0 animate-spin text-primary" aria-hidden />
        {t(`updates.progress.${phase}`)}
      </p>

      {phase === "downloading" ? (
        <>
          {percent !== null ? (
            <div
              role="progressbar"
              aria-label={t("updates.progress.barLabel")}
              aria-valuemin={0}
              aria-valuemax={PERCENT_MAX}
              aria-valuenow={percent}
              className="h-2 w-full overflow-hidden rounded-full bg-surface-3"
            >
              <div
                className="h-full rounded-full bg-primary transition-[width] duration-normal"
                style={{ width: `${percent}%` }}
              />
            </div>
          ) : null}
          <p className="text-xs text-text-secondary">
            {percent !== null && knownTotal !== null
              ? `${t("updates.progress.percent", { percent })} · ${t(
                  "updates.progress.megabytesOfTotal",
                  {
                    downloaded: downloadedMb,
                    total: megabytes.format(knownTotal / BYTES_PER_MEGABYTE),
                  },
                )}`
              : t("updates.progress.megabytes", { downloaded: downloadedMb })}
          </p>
        </>
      ) : null}

      <p className="text-xs text-text-muted">{t("updates.progress.keepOpen")}</p>
    </div>
  );
}

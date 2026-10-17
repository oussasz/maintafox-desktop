import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";

export function StatusBar() {
  const { t } = useTranslation("shell");
  const isOnline = useAppStore((s) => s.isOnline);
  const syncStatus = useAppStore((s) => s.syncStatus);
  const appVersion = useAppStore((s) => s.appVersion);

  return (
    <footer
      className="fixed inset-x-0 bottom-0 z-30 flex h-statusbar items-center
                 justify-between border-t border-surface-border bg-surface-0
                 px-3 text-2xs text-text-muted select-none"
    >
      {/* Left: connectivity */}
      <div className="flex items-center gap-3">
        <span className="flex items-center gap-1">
          <span
            className={cn(
              "inline-block h-1.5 w-1.5 rounded-full",
              isOnline ? "bg-tone-success-fg" : "bg-tone-warning-fg",
            )}
            aria-hidden
          />
          {isOnline ? t("status.online") : t("status.offline")}
        </span>

        {syncStatus.pendingCount > 0 && (
          <span className="text-tone-warning-fg">
            {t("status.pendingSync", { count: syncStatus.pendingCount })}
          </span>
        )}
        {syncStatus.state !== "idle" && (
          <span className="text-text-muted">
            {t("sync.indicator.statusBar", { state: t(`sync.indicator.${syncStatus.state}`) })}
          </span>
        )}
        {syncStatus.blockerReason && (
          <span className="max-w-72 truncate text-tone-warning-fg" title={syncStatus.blockerReason}>
            {syncStatus.blockerReason}
          </span>
        )}
      </div>

      {/* Right: version. No database indicator: nothing checks the database while the app
          runs, and a dot that is always green would claim a health it does not know. */}
      <div className="flex items-center gap-3">
        {appVersion && <span>{t("status.version", { version: appVersion })}</span>}
      </div>
    </footer>
  );
}

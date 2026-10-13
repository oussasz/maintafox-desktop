import { useState } from "react";
import { useTranslation } from "react-i18next";

import { UpdateStatusDialog } from "@/components/updates/UpdateStatusDialog";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";
import { useUpdaterStore } from "@/stores/updater-store";

export function StatusBar() {
  const { t } = useTranslation("shell");
  const isOnline = useAppStore((s) => s.isOnline);
  const syncStatus = useAppStore((s) => s.syncStatus);
  const appVersion = useAppStore((s) => s.appVersion);
  const updateAvailable = useUpdaterStore((s) => s.lastCheckResult?.available === true);
  const [updatesOpen, setUpdatesOpen] = useState(false);

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
              isOnline ? "bg-status-success" : "bg-status-warning",
            )}
          />
          {isOnline ? t("status.online") : t("status.offline")}
        </span>

        {syncStatus.pendingCount > 0 && (
          <span className="text-status-warning">
            {t("status.pendingSync", { count: syncStatus.pendingCount })}
          </span>
        )}
        {syncStatus.state !== "idle" && (
          <span className="text-text-muted">
            {t("sync.indicator.statusBar", { state: t(`sync.indicator.${syncStatus.state}`) })}
          </span>
        )}
        {syncStatus.blockerReason && (
          <span className="text-status-warning max-w-72 truncate" title={syncStatus.blockerReason}>
            {syncStatus.blockerReason}
          </span>
        )}
      </div>

      {/* Right: db health + version */}
      <div className="flex items-center gap-3">
        <span title={t("status.dbHealthy")}>
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-status-success mr-1" />
          {t("status.db")}
        </span>
        {appVersion && (
          <button
            type="button"
            className="flex items-center gap-1 hover:text-text-primary"
            onClick={() => setUpdatesOpen(true)}
            title={t("updates.status.open", { version: appVersion })}
            aria-label={t("updates.status.open", { version: appVersion })}
          >
            {updateAvailable && (
              <span className="inline-block h-1.5 w-1.5 rounded-full bg-primary" aria-hidden />
            )}
            v{appVersion}
          </button>
        )}
      </div>
      <UpdateStatusDialog
        open={updatesOpen}
        onOpenChange={setUpdatesOpen}
        appVersion={appVersion}
      />
    </footer>
  );
}

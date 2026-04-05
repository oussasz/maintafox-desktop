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
      </div>

      {/* Right: db health + version */}
      <div className="flex items-center gap-3">
        <span title={t("status.dbHealthy")}>
          <span className="inline-block h-1.5 w-1.5 rounded-full bg-status-success mr-1" />
          {t("status.db")}
        </span>
        {appVersion && <span>v{appVersion}</span>}
      </div>
    </footer>
  );
}

import { useState } from "react";
import { useTranslation } from "react-i18next";

import { UpdateStatusDialog } from "@/components/updates/UpdateStatusDialog";
import { type DbHealth, useDbHealth } from "@/hooks/use-db-health";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";
import { useUpdaterStore } from "@/stores/updater-store";

export function StatusBar() {
  const { t } = useTranslation("shell");
  const isOnline = useAppStore((s) => s.isOnline);
  const syncStatus = useAppStore((s) => s.syncStatus);
  const appVersion = useAppStore((s) => s.appVersion);
  const db = useDbHealth();
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

      {/* Right: database state (the backend's real probe) and version. */}
      <div className="flex items-center gap-3">
        <DbIndicator db={db} />
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
            {t("status.version", { version: appVersion })}
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

const DB_DOT: Record<DbHealth["state"], string> = {
  checking: "bg-tone-neutral-fg",
  ok: "bg-tone-success-fg",
  degraded: "bg-tone-danger-fg",
  unknown: "bg-tone-warning-fg",
};

/** Database state as the probe said it: degraded stays visible; unknown is never shown as green. */
function DbIndicator({ db }: { db: DbHealth }) {
  const { t } = useTranslation("shell");
  const text =
    db.state === "ok"
      ? db.latencyMs != null
        ? t("status.dbOkLatency", { ms: db.latencyMs })
        : t("status.dbOk")
      : db.state === "degraded"
        ? t(db.errorCode === "DB_TIMEOUT" ? "status.dbTimeout" : "status.dbError")
        : db.state === "unknown"
          ? t("status.dbUnknown")
          : t("status.dbChecking");
  return (
    <span
      role="status"
      data-testid="db-health"
      data-state={db.state}
      className={cn(
        "flex items-center gap-1",
        db.state === "degraded" && "font-medium text-tone-danger-fg",
        db.state === "unknown" && "text-tone-warning-fg",
      )}
    >
      <span className={cn("inline-block h-1.5 w-1.5 rounded-full", DB_DOT[db.state])} aria-hidden />
      {text}
    </span>
  );
}

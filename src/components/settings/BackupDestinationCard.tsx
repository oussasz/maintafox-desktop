import { AlertTriangle, FolderOpen, HardDrive, Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { useFormatters } from "@/hooks/use-formatters";
import { isStepUpCancelled } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import { getBackupOverview, setBackupExternalDir } from "@/services/backup-service";
import type { BackupOverview } from "@shared/ipc-types";

type Toast = (msg: {
  title: string;
  description?: string;
  variant?: "default" | "destructive" | "success";
}) => void;

/**
 * Backups outside this PC (L7-03): the folder scheduled backups go to (USB disk, network share),
 * a warning when no recent copy exists outside the PC, and how to restore on another PC.
 */
export function BackupDestinationCard({
  onToast,
  withStepUp,
  refreshKey,
}: {
  onToast: Toast;
  withStepUp: <T>(fn: () => Promise<T>) => Promise<T>;
  /** Changes after a backup is made, to read the status again. */
  refreshKey: number;
}) {
  const { t } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const [overview, setOverview] = useState<BackupOverview | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(async () => {
    try {
      setOverview(await getBackupOverview());
    } catch (err) {
      onToast({
        title: t("backupDest.loadError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    }
  }, [onToast, t]);

  useEffect(() => {
    void reload();
  }, [reload, refreshKey]);

  const change = useCallback(
    async (path: string | null) => {
      setBusy(true);
      try {
        await withStepUp(() => setBackupExternalDir(path));
        onToast({
          title: path ? t("backupDest.saved") : t("backupDest.cleared"),
          variant: "success",
        });
        await reload();
      } catch (err) {
        if (isStepUpCancelled(err)) return;
        onToast({
          title: t("backupDest.saveError"),
          description: appErrorReason(err),
          variant: "destructive",
        });
      } finally {
        setBusy(false);
      }
    },
    [onToast, reload, t, withStepUp],
  );

  const choose = useCallback(async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      title: t("backupDest.chooseTitle"),
      directory: true,
      multiple: false,
    });
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (path && typeof path === "string") await change(path);
  }, [change, t]);

  const dataDir = overview?.app_backup_dir.replace(/[\\/]backups[\\/]?$/, "") ?? "";

  return (
    <div className="space-y-3 rounded-md border border-border p-3">
      <h4 className="flex items-center gap-2 text-sm font-medium">
        <HardDrive className="h-4 w-4" />
        {t("backupDest.title")}
      </h4>
      <p className="text-xs text-text-muted">{t("backupDest.description")}</p>

      {overview?.outside_copy_missing_or_old ? (
        <div
          role="alert"
          className="flex gap-2 rounded-md border border-tone-warning-border bg-tone-warning-bg p-2 text-sm text-tone-warning-fg"
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <span>
            {overview.last_outside_at
              ? t("backupDest.outsideOld", {
                  date: formatDateTime(overview.last_outside_at),
                  days: overview.outside_copy_max_age_days,
                })
              : t("backupDest.outsideNone")}
          </span>
        </div>
      ) : overview?.last_outside_at ? (
        <p className="text-sm">
          {t("backupDest.outsideOk", { date: formatDateTime(overview.last_outside_at) })}
        </p>
      ) : null}

      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-text-muted">{t("backupDest.folder")}</span>
        <span className="break-all font-medium">
          {overview?.external_dir ?? t("backupDest.folderNone")}
        </span>
        {overview?.external_dir && !overview.external_reachable ? (
          <span className="text-tone-warning-fg">{t("backupDest.unreachable")}</span>
        ) : null}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" onClick={() => void choose()} disabled={busy}>
          {busy ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <FolderOpen className="mr-2 h-4 w-4" />
          )}
          {overview?.external_dir ? t("backupDest.change") : t("backupDest.choose")}
        </Button>
        {overview?.external_dir ? (
          <Button type="button" variant="ghost" onClick={() => void change(null)} disabled={busy}>
            {t("backupDest.clear")}
          </Button>
        ) : null}
      </div>

      <details className="rounded-md bg-muted/40 p-2 text-sm">
        <summary className="cursor-pointer font-medium">{t("backupDest.guideTitle")}</summary>
        <div className="mt-2 space-y-2">
          <p className="font-medium">{t("backupDest.guideSamePcTitle")}</p>
          <p>{t("backupDest.guideSamePc")}</p>
          <p className="font-medium">{t("backupDest.guideNewPcTitle")}</p>
          <ol className="list-decimal space-y-1 pl-5">
            <li>{t("backupDest.guideNewPc1")}</li>
            <li>{t("backupDest.guideNewPc2")}</li>
            <li>{t("backupDest.guideNewPc3", { dataDir })}</li>
            <li>{t("backupDest.guideNewPc4")}</li>
          </ol>
        </div>
      </details>
    </div>
  );
}

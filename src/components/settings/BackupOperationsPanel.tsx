import {
  ArchiveRestore,
  HardDriveDownload,
  History,
  Loader2,
  RotateCcw,
  Search,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
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
import { useStepUp, isStepUpCancelled } from "@/hooks/use-step-up";
import { appErrorReason } from "@/lib/app-error";
import {
  factoryReset,
  listBackupRuns,
  restoreFromBackup,
  runManualBackup,
  validateBackupFile,
} from "@/services/backup-service";
import type { BackupRunRecord, RestoreTestResult } from "@shared/ipc-types";

interface BackupOperationsPanelProps {
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

/** The confirmation phrases the backend accepts (`commands::backup`), shown to the user. */
const FACTORY_RESET_PHRASES = ["EFFACER TOUTES LES DONNÉES", "ERASE ALL DATA"];

export function BackupOperationsPanel({ onToast }: BackupOperationsPanelProps) {
  const { t } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const formatRunStatus = useCallback(
    (status: string) => {
      const key = `backupOps.status.${status}` as "backupOps.status.success";
      const translated = t(key);
      return translated === key ? status : translated;
    },
    [t],
  );

  const formatTrigger = useCallback(
    (trigger: string) => {
      const key = `backupOps.trigger.${trigger}` as "backupOps.trigger.manual";
      const translated = t(key);
      return translated === key ? trigger : translated;
    },
    [t],
  );

  const [runs, setRuns] = useState<BackupRunRecord[]>([]);
  const [loadingRuns, setLoadingRuns] = useState(true);
  const [busy, setBusy] = useState(false);
  const [validation, setValidation] = useState<RestoreTestResult | null>(null);
  const [resetPhrase, setResetPhrase] = useState("");
  const [restorePath, setRestorePath] = useState<string | null>(null);

  const reloadRuns = useCallback(async () => {
    setLoadingRuns(true);
    try {
      setRuns(await listBackupRuns(20));
    } catch (err) {
      onToast({
        title: t("backupOps.loadError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setLoadingRuns(false);
    }
  }, [onToast, t]);

  useEffect(() => {
    void reloadRuns();
  }, [reloadRuns]);

  const handleRunBackup = useCallback(async () => {
    setBusy(true);
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        title: t("backupOps.chooseTarget"),
        defaultPath: `maintafox-backup-${new Date().toISOString().slice(0, 10)}.db`,
        filters: [{ name: "SQLite", extensions: ["db", "sqlite"] }],
      });
      if (!target || typeof target !== "string") return;

      const result = await withStepUp(() => runManualBackup(target));
      onToast({
        title: t("backupOps.runSuccess"),
        description: result.output_path,
        variant: "success",
      });
      void reloadRuns();
    } catch (err) {
      if (isStepUpCancelled(err)) return;
      onToast({
        title: t("backupOps.runError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  }, [onToast, reloadRuns, t, withStepUp]);

  const handleValidate = useCallback(async () => {
    setBusy(true);
    setValidation(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title: t("backupOps.chooseBackup"),
        multiple: false,
        filters: [{ name: "SQLite", extensions: ["db", "sqlite"] }],
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (!path || typeof path !== "string") return;

      const result = await validateBackupFile(path);
      setValidation(result);
      onToast({
        title: result.checksum_match ? t("backupOps.validateOk") : t("backupOps.validateMismatch"),
        variant: result.checksum_match ? "success" : "destructive",
      });
    } catch (err) {
      onToast({
        title: t("backupOps.validateError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  }, [onToast, t]);

  const handleChooseRestore = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title: t("backupOps.chooseRestore"),
        multiple: false,
        filters: [{ name: "SQLite", extensions: ["db", "sqlite"] }],
      });
      const path = Array.isArray(selected) ? selected[0] : selected;
      if (path && typeof path === "string") setRestorePath(path);
    } catch (err) {
      onToast({
        title: t("backupOps.restoreError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    }
  }, [onToast, t]);

  const handleConfirmRestore = useCallback(async () => {
    if (!restorePath) return;
    const path = restorePath;
    setRestorePath(null);
    setBusy(true);
    try {
      // On success the application restarts; this call does not return.
      await withStepUp(() => restoreFromBackup(path));
    } catch (err) {
      if (isStepUpCancelled(err)) return;
      onToast({
        title: t("backupOps.restoreError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  }, [onToast, restorePath, t, withStepUp]);

  const handleFactoryReset = useCallback(async () => {
    setBusy(true);
    try {
      // On success the application restarts; this call does not return.
      await withStepUp(() => factoryReset(resetPhrase.trim()));
    } catch (err) {
      if (isStepUpCancelled(err)) return;
      onToast({
        title: t("backupOps.resetError"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  }, [onToast, resetPhrase, t, withStepUp]);

  return (
    <div className="space-y-4 rounded-lg border border-border p-4">
      <div>
        <h3 className="text-base font-semibold">{t("backupOps.title")}</h3>
        <p className="text-sm text-text-muted">{t("backupOps.description")}</p>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button type="button" onClick={() => void handleRunBackup()} disabled={busy}>
          {busy ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <HardDriveDownload className="mr-2 h-4 w-4" />
          )}
          {t("backupOps.run")}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => void handleValidate()}
          disabled={busy}
        >
          <Search className="mr-2 h-4 w-4" />
          {t("backupOps.validate")}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => void handleChooseRestore()}
          disabled={busy}
        >
          <ArchiveRestore className="mr-2 h-4 w-4" />
          {t("backupOps.restore")}
        </Button>
      </div>

      {validation ? (
        <div className="rounded-md border border-border bg-muted/40 p-3 text-sm">
          <p>
            {t("backupOps.checksumMatch")}:{" "}
            <strong>{validation.checksum_match ? t("backupOps.yes") : t("backupOps.no")}</strong>
          </p>
          <p className="truncate text-text-muted">
            {t("backupOps.computedChecksum")}: {validation.computed_checksum}
          </p>
          {validation.warnings.length > 0 ? (
            <ul className="mt-2 list-disc pl-5 text-tone-warning-fg">
              {validation.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      <div>
        <h4 className="mb-2 flex items-center gap-2 text-sm font-medium">
          <History className="h-4 w-4" />
          {t("backupOps.history")}
        </h4>
        {loadingRuns ? (
          <p className="text-sm text-text-muted">{t("page.loading")}</p>
        ) : runs.length === 0 ? (
          <p className="text-sm text-text-muted">{t("backupOps.historyEmpty")}</p>
        ) : (
          <div className="max-h-48 overflow-auto rounded-md border border-border">
            <table className="w-full text-sm">
              <thead className="bg-muted/50 text-left text-text-muted">
                <tr>
                  <th className="px-3 py-2">{t("backupOps.colStarted")}</th>
                  <th className="px-3 py-2">{t("backupOps.colTrigger")}</th>
                  <th className="px-3 py-2">{t("backupOps.colStatus")}</th>
                  <th className="px-3 py-2">{t("backupOps.colPath")}</th>
                </tr>
              </thead>
              <tbody>
                {runs.map((run) => (
                  <tr key={run.id} className="border-t border-border">
                    <td className="whitespace-nowrap px-3 py-2">
                      {formatDateTime(run.started_at)}
                    </td>
                    <td className="px-3 py-2">{formatTrigger(run.trigger)}</td>
                    <td className="px-3 py-2" title={run.error_message ?? undefined}>
                      {formatRunStatus(run.status)}
                    </td>
                    <td className="max-w-[14rem] truncate px-3 py-2" title={run.output_path}>
                      {run.output_path}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="space-y-2 rounded-md border border-destructive/30 bg-destructive/5 p-3">
        <h4 className="flex items-center gap-2 text-sm font-medium text-destructive">
          <RotateCcw className="h-4 w-4" />
          {t("backupOps.resetTitle")}
        </h4>
        <p className="text-xs text-text-muted">{t("backupOps.resetDescription")}</p>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <div className="flex-1 space-y-1">
            <Label htmlFor="factory-reset-phrase">{t("backupOps.resetPhraseLabel")}</Label>
            <Input
              id="factory-reset-phrase"
              value={resetPhrase}
              onChange={(e) => setResetPhrase(e.target.value)}
              placeholder={t("backupOps.resetPhrasePlaceholder")}
            />
          </div>
          <Button
            type="button"
            variant="destructive"
            disabled={busy || !FACTORY_RESET_PHRASES.includes(resetPhrase.trim())}
            onClick={() => void handleFactoryReset()}
          >
            {t("backupOps.resetAction")}
          </Button>
        </div>
      </div>

      <Dialog open={restorePath !== null} onOpenChange={(o) => !o && setRestorePath(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("backupOps.restoreConfirmTitle")}</DialogTitle>
            <DialogDescription>{t("backupOps.restoreConfirmBody")}</DialogDescription>
          </DialogHeader>
          <p className="break-all text-sm text-text-muted">{restorePath}</p>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setRestorePath(null)}>
              {t("backupOps.cancel")}
            </Button>
            <Button type="button" variant="destructive" onClick={() => void handleConfirmRestore()}>
              {t("backupOps.restoreConfirmAction")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {StepUpDialogElement}
    </div>
  );
}

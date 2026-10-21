import { Loader2, RefreshCw } from "lucide-react";
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
import { useUpdater } from "@/hooks/use-updater";
import type { UpdateCheckResult } from "@shared/ipc-types";

interface UpdateStatusDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Version reported by the app (shown before the first check answers). */
  appVersion: string;
}

function StatusText({ result }: { result: UpdateCheckResult }) {
  const { t } = useTranslation("shell");
  switch (result.status) {
    case "available":
      return <>{t("updates.status.available", { version: result.version ?? "" })}</>;
    case "up_to_date":
      return <>{t("updates.status.up_to_date")}</>;
    case "not_configured":
      return <>{t("updates.status.not_configured")}</>;
    case "not_activated":
      return <>{t("updates.status.not_activated")}</>;
    case "unreachable":
      return <>{t("updates.status.unreachable")}</>;
  }
}

/** "Check for updates": installed version, last check status, manual check. */
export function UpdateStatusDialog({ open, onOpenChange, appVersion }: UpdateStatusDialogProps) {
  const { t } = useTranslation("shell");
  const u = useUpdater();
  const version = u.result?.current_version ?? appVersion;
  const technical = u.result?.status === "unreachable" ? u.result.message : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("updates.status.title")}</DialogTitle>
          <DialogDescription>{t("updates.status.currentVersion", { version })}</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-1 text-sm text-text-primary" aria-live="polite">
          {u.checkError ? (
            <>
              <p role="alert">{t("updates.error.check")}</p>
              <p className="text-xs text-text-muted">
                {t("updates.error.detail", { detail: u.checkError })}
              </p>
            </>
          ) : u.result ? (
            <>
              <p>
                <StatusText result={u.result} />
              </p>
              {technical ? (
                <p className="text-xs text-text-muted">
                  {t("updates.error.detail", { detail: technical })}
                </p>
              ) : null}
            </>
          ) : (
            <p className="text-text-secondary">{t("updates.status.notCheckedYet")}</p>
          )}
        </div>
        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("updates.status.close")}
          </Button>
          <Button onClick={u.checkNow} disabled={u.isChecking || u.phase !== "idle"}>
            {u.isChecking ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="mr-2 h-4 w-4" aria-hidden />
            )}
            {u.isChecking ? t("updates.status.checking") : t("updates.status.check")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

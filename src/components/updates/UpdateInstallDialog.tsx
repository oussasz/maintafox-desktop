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

interface UpdateInstallDialogProps {
  open: boolean;
  version: string;
  /** Maintafox withdrew the running version: an older one is reinstalled. */
  isRollback: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Confirmation before an in-app install: backup first, the app restarts, unsaved work is lost. */
export function UpdateInstallDialog({
  open,
  version,
  isRollback,
  onCancel,
  onConfirm,
}: UpdateInstallDialogProps) {
  const { t } = useTranslation("shell");

  return (
    <Dialog open={open} onOpenChange={(next) => (!next ? onCancel() : undefined)}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isRollback
              ? t("updates.confirm.titleRollback", { version })
              : t("updates.confirm.title", { version })}
          </DialogTitle>
          <DialogDescription>{t("updates.confirm.intro")}</DialogDescription>
        </DialogHeader>
        <ul className="list-disc space-y-1 pl-5 text-sm text-text-primary">
          <li>{t("updates.confirm.backup")}</li>
          <li>{t("updates.confirm.restart")}</li>
          <li className="font-medium">{t("updates.confirm.unsaved")}</li>
        </ul>
        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={onCancel}>
            {t("updates.confirm.cancel")}
          </Button>
          <Button onClick={onConfirm}>{t("updates.confirm.confirm")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

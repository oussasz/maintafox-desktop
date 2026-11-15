/**
 * Names a reliability study when it is created, or renames it (L2-13). On creation the name may
 * stay empty: the backend names the study after its equipment, mode and creation day. A selection
 * of equipment of different classes is created only once the person confirms it (L2-18); the
 * backend refuses it otherwise.
 */
import { useEffect, useState } from "react";
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

import { RamsCallout } from "./rams-ui";

export function StudyNameDialog({
  open,
  onOpenChange,
  mode,
  initialName = "",
  mixedClasses = [],
  onSubmit,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: "create" | "rename";
  initialName?: string;
  /** Class names of a mixed selection (empty: one class). */
  mixedClasses?: string[];
  /** Runs the creation or the rename; a refusal keeps the dialog open. */
  onSubmit: (name: string, acceptMixedClasses: boolean) => Promise<void>;
}) {
  const { t } = useTranslation("reliability");
  const [name, setName] = useState(initialName);
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName(initialName);
    setAck(false);
  }, [open, initialName]);

  const mixed = mixedClasses.length > 1;
  const valid = (mode === "create" || name.trim().length > 0) && (!mixed || ack);

  const submit = async () => {
    if (!valid) return;
    setBusy(true);
    try {
      await onSubmit(name.trim(), mixed && ack);
      onOpenChange(false);
    } catch {
      // The global IPC toast reports the refusal; the dialog stays open.
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !busy && onOpenChange(o)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {mode === "create" ? t("rams.studyName.createTitle") : t("rams.studyName.renameTitle")}
          </DialogTitle>
          {mode === "create" ? (
            <DialogDescription>{t("rams.studyName.createHint")}</DialogDescription>
          ) : null}
        </DialogHeader>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="grid gap-1">
            <label className="grid gap-1 text-xs font-medium text-text-secondary">
              {t("rams.studyName.name")}
              <Input
                className="h-10"
                value={name}
                autoFocus
                maxLength={200}
                aria-describedby={mode === "create" ? "rams-study-name-hint" : undefined}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            {mode === "create" ? (
              <p id="rams-study-name-hint" className="text-xs text-text-muted">
                {t("rams.studyName.autoHint")}
              </p>
            ) : null}
          </div>
          {mixed ? (
            <RamsCallout tone="warning" title={t("rams.fleet.mixedClassesTitle")}>
              <p>{t("rams.studyName.mixedBody", { classes: mixedClasses.join(", ") })}</p>
              <label className="mt-2 flex min-h-[36px] items-start gap-2 text-sm text-text-primary">
                <input
                  type="checkbox"
                  className="mt-0.5 h-4 w-4 accent-primary"
                  checked={ack}
                  onChange={(e) => setAck(e.target.checked)}
                />
                {t("rams.studyName.mixedAck")}
              </label>
            </RamsCallout>
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              disabled={busy}
              onClick={() => onOpenChange(false)}
            >
              {t("rams.common.cancel")}
            </Button>
            <Button type="submit" className="h-11" disabled={!valid || busy}>
              {mode === "create" ? t("rams.studyName.create") : t("rams.studyName.renameAction")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

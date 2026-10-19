/**
 * WoResumeDialog.tsx
 *
 * Confirms the resume of a work order on hold, with the optional estimated remaining time used
 * by the permit gate instead of the full expected duration (see WoRemainingHoursField).
 */

import { Play } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

import { WoRemainingHoursField, parseRemainingHours } from "./WoRemainingHoursField";

interface WoResumeDialogProps {
  open: boolean;
  busy: boolean;
  onOpenChange: (open: boolean) => void;
  /** `remainingHours`: the estimated remaining time entered, or `null` (full expected duration). */
  onConfirm: (remainingHours: number | null) => void;
}

export function WoResumeDialog({ open, busy, onOpenChange, onConfirm }: WoResumeDialogProps) {
  const { t } = useTranslation("ot");
  const [remaining, setRemaining] = useState("");

  useEffect(() => {
    if (open) setRemaining("");
  }, [open]);

  const parsed = parseRemainingHours(remaining);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("resumeDialog.title")}</DialogTitle>
          <DialogDescription>{t("resumeDialog.description")}</DialogDescription>
        </DialogHeader>
        <WoRemainingHoursField
          id="wo-resume-remaining-hours"
          value={remaining}
          onChange={setRemaining}
          disabled={busy}
        />
        <div className="flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)} disabled={busy}>
            {t("resumeDialog.cancel")}
          </Button>
          <Button
            size="sm"
            className="gap-1.5"
            disabled={busy || parsed === "invalid"}
            onClick={() => {
              if (parsed === "invalid") return;
              onConfirm(parsed);
            }}
          >
            <Play className="h-3.5 w-3.5" />
            {t("footer.resume")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

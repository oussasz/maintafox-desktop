/**
 * DiDeferDialog.tsx
 *
 * Defers a DI until a date the reviewer chooses, with a reason and an optional note.
 * The backend (`defer_di`) requires a reason and a date strictly after today.
 */

import { CalendarClock, Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { Button } from "@/components/ui/button";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useDiReviewStore } from "@/stores/di-review-store";
import type { InterventionRequest } from "@shared/ipc-types";

/** Local calendar date `YYYY-MM-DD`, `offsetDays` from today. */
export function localIsoDate(offsetDays = 0, from: Date = new Date()): string {
  const d = new Date(from.getFullYear(), from.getMonth(), from.getDate() + offsetDays);
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

interface DiDeferDialogProps {
  di: InterventionRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onDeferred?: () => void;
}

export function DiDeferDialog({ di, open, onOpenChange, onDeferred }: DiDeferDialogProps) {
  const { t } = useTranslation("di");
  const defer = useDiReviewStore((s) => s.defer);
  const minDate = useMemo(() => localIsoDate(1), []);
  const [until, setUntil] = useState("");
  /** Code of the plant's deferral reason list (`DI.DEFERRAL_REASON`). */
  const [reason, setReason] = useState<string | null>(null);
  const [notes, setNotes] = useState("");
  const [touched, setTouched] = useState(false);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!open) return;
    setUntil("");
    setReason(null);
    setNotes("");
    setTouched(false);
  }, [open]);

  if (!di) return null;

  const dateError = !until
    ? t("deferral.dateRequired")
    : until < minDate
      ? t("deferral.dateFuture")
      : null;
  const reasonError = !reason ? t("deferral.reasonRequired") : null;
  const notesError =
    reason === "other" && !notes.trim() ? t("deferral.notesRequiredForOther") : null;
  const valid = !dateError && !reasonError && !notesError;

  const handleSubmit = async () => {
    setTouched(true);
    if (!valid) return;
    setPending(true);
    try {
      await defer({
        di_id: di.id,
        actor_id: 0, // the backend uses the session user
        expected_row_version: di.row_version,
        deferred_until: `${until}T00:00:00Z`,
        reason_code: reason ?? "",
        notes: notes.trim() || null,
      });
      onOpenChange(false);
      onDeferred?.();
    } catch {
      // The global IPC toast reports the reason; keep the dialog open.
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{t("deferral.title", { code: di.code })}</DialogTitle>
          <DialogDescription>{t("deferral.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label htmlFor="di-defer-until">{t("deferral.until")}</Label>
            <DateField
              id="di-defer-until"
              className="h-11"
              min={minDate}
              value={until}
              aria-invalid={touched && dateError != null}
              aria-describedby={touched && dateError ? "di-defer-until-error" : undefined}
              onChange={(v) => setUntil(v)}
            />
            {touched && dateError ? (
              <p id="di-defer-until-error" className="text-xs text-tone-danger-fg">
                {dateError}
              </p>
            ) : null}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="di-defer-reason">{t("deferral.reason")}</Label>
            <ReferenceCombobox
              id="di-defer-reason"
              referenceType="di.deferral_reason"
              value={reason}
              onChange={setReason}
              allowClear={false}
              allowCreate={false}
              placeholder={t("deferral.reasonPlaceholder")}
              aria-invalid={touched && reasonError != null}
            />
            {touched && reasonError ? (
              <p className="text-xs text-tone-danger-fg">{reasonError}</p>
            ) : null}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="di-defer-notes">{t("deferral.notes")}</Label>
            <Textarea
              id="di-defer-notes"
              rows={3}
              value={notes}
              placeholder={t("deferral.notesPlaceholder")}
              onChange={(e) => setNotes(e.target.value)}
            />
            {touched && notesError ? (
              <p className="text-xs text-tone-danger-fg">{notesError}</p>
            ) : null}
          </div>
        </div>

        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            disabled={pending}
            onClick={() => onOpenChange(false)}
          >
            {t("deferral.cancel")}
          </Button>
          <Button className="h-11 gap-1.5" disabled={pending} onClick={() => void handleSubmit()}>
            {pending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <CalendarClock className="h-4 w-4" aria-hidden />
            )}
            {t("deferral.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

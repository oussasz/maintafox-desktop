/**
 * DiReturnDialog.tsx
 *
 * Return-for-clarification dialog with required note.
 * Small dialog (max-w-lg).
 *
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S4.
 */

import { Info, Loader2 } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { useDiReviewStore } from "@/stores/di-review-store";

// ── Component ───────────────────────────────────────────────────────────────

export function DiReturnDialog() {
  const { t } = useTranslation("di");
  const di = useDiReviewStore((s) => s.returnDi_);
  const closeReturn = useDiReviewStore((s) => s.closeReturn);
  const returnForClarification = useDiReviewStore((s) => s.returnForClarification);
  const saving = useDiReviewStore((s) => s.saving);
  const { info } = useSession();

  const [note, setNote] = useState("");
  const [touched, setTouched] = useState(false);

  const open = di !== null;
  const noteError = touched && !note.trim();

  const handleReturn = useCallback(async () => {
    setTouched(true);
    if (!di || !note.trim()) return;
    try {
      await returnForClarification({
        di_id: di.id,
        actor_id: info?.user_id ?? 0,
        expected_row_version: di.row_version,
        reviewer_note: note.trim(),
      });
      setNote("");
      setTouched(false);
      closeReturn();
    } catch {
      // error handled by store
    }
  }, [di, returnForClarification, info, note, closeReturn]);

  const handleClose = useCallback(() => {
    setNote("");
    setTouched(false);
    closeReturn();
  }, [closeReturn]);

  if (!di) return null;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && handleClose()}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="text-lg font-bold">{t("review.returnTitle")}</DialogTitle>
        </DialogHeader>

        <Separator />

        <div className="space-y-4 py-2">
          {/* DI summary */}
          <div className="text-sm space-y-1">
            <div className="flex items-center gap-2">
              <span className="font-mono text-muted-foreground">{di.code}</span>
              <span className="font-semibold truncate">{di.title}</span>
            </div>
            <div className="text-xs text-muted-foreground">
              {t("detail.fields.reportedBy")}: #{di.submitter_id}
            </div>
          </div>

          {/* Clarification note (required) */}
          <div className="space-y-1.5">
            <Label htmlFor="return-note">
              {t("review.clarificationNote")} <span className="text-red-500">*</span>
            </Label>
            <Textarea
              id="return-note"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              onBlur={() => setTouched(true)}
              maxLength={2000}
              placeholder={t("review.clarificationNotePlaceholder")}
              rows={4}
              className={noteError ? "border-red-500" : ""}
            />
            {noteError && (
              <p className="text-[11px] text-red-600">{t("review.clarificationRequired")}</p>
            )}
            <p className="text-[10px] text-muted-foreground text-right">{note.length}/2000</p>
          </div>

          {/* Info banner */}
          <div className="flex items-center gap-2 rounded-md bg-amber-50 border border-amber-200 px-3 py-2 text-xs text-amber-700">
            <Info className="h-4 w-4 shrink-0" />
            <span>{t("review.returnInfo")}</span>
          </div>
        </div>

        <Separator />

        <DialogFooter className="flex items-center justify-end gap-2">
          <Button variant="outline" size="sm" onClick={handleClose}>
            {t("form.cancel")}
          </Button>
          <Button
            size="sm"
            onClick={handleReturn}
            disabled={saving || (!note.trim() && touched)}
            className="gap-1.5 bg-amber-600 hover:bg-amber-700 text-white"
          >
            {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("review.returnAction")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

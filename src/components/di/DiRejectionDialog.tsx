/**
 * DiRejectionDialog.tsx
 *
 * Rejection confirmation modal with DI summary, reason textarea, and warning.
 * Small dialog (max-w-lg).
 *
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S4.
 */

import { AlertTriangle, Loader2 } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { useDiReviewStore } from "@/stores/di-review-store";

const URGENCY_STYLE: Record<string, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-800",
  medium: "bg-yellow-100 text-yellow-800",
  low: "bg-green-100 text-green-800",
};

// ── Component ───────────────────────────────────────────────────────────────

export function DiRejectionDialog() {
  const { t } = useTranslation("di");
  const di = useDiReviewStore((s) => s.rejectionDi);
  const closeRejection = useDiReviewStore((s) => s.closeRejection);
  const reject = useDiReviewStore((s) => s.reject);
  const saving = useDiReviewStore((s) => s.saving);
  const { info } = useSession();

  const [reasonCode, setReasonCode] = useState("");
  const [notes, setNotes] = useState("");
  const [touched, setTouched] = useState(false);

  const open = di !== null;
  const reasonError = touched && !reasonCode.trim();

  const handleReject = useCallback(async () => {
    setTouched(true);
    if (!di || !reasonCode.trim()) return;
    try {
      await reject({
        di_id: di.id,
        actor_id: info?.user_id ?? 0,
        expected_row_version: di.row_version,
        reason_code: reasonCode.trim(),
        notes: notes.trim() || null,
      });
      setReasonCode("");
      setNotes("");
      setTouched(false);
      closeRejection();
    } catch {
      // error handled by store
    }
  }, [di, reject, info, reasonCode, notes, closeRejection]);

  const handleClose = useCallback(() => {
    setReasonCode("");
    setNotes("");
    setTouched(false);
    closeRejection();
  }, [closeRejection]);

  if (!di) return null;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && handleClose()}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="text-lg font-bold">{t("review.rejectionTitle")}</DialogTitle>
        </DialogHeader>

        <Separator />

        <div className="space-y-4 py-2">
          {/* DI summary */}
          <div className="flex items-center gap-2 text-sm">
            <span className="font-mono text-muted-foreground">{di.code}</span>
            <span className="font-semibold truncate">{di.title}</span>
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ml-auto ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
            >
              {t(`priority.${di.reported_urgency}`)}
            </Badge>
          </div>

          {/* Reason code (required) */}
          <div className="space-y-1.5">
            <Label htmlFor="rejection-reason-code">
              {t("review.reasonCode")} <span className="text-red-500">*</span>
            </Label>
            <Input
              id="rejection-reason-code"
              value={reasonCode}
              onChange={(e) => setReasonCode(e.target.value)}
              onBlur={() => setTouched(true)}
              placeholder={t("review.reasonCodePlaceholder")}
              className={reasonError ? "border-red-500" : ""}
            />
            {reasonError && (
              <p className="text-[11px] text-red-600">{t("review.reasonCodeRequired")}</p>
            )}
          </div>

          {/* Notes */}
          <div className="space-y-1.5">
            <Label htmlFor="rejection-notes">{t("review.rejectionNotes")}</Label>
            <Textarea
              id="rejection-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              maxLength={2000}
              placeholder={t("review.rejectionNotesPlaceholder")}
              rows={3}
            />
            <p className="text-[10px] text-muted-foreground text-right">{notes.length}/2000</p>
          </div>

          {/* Warning */}
          <div className="flex items-center gap-2 rounded-md bg-red-50 border border-red-200 px-3 py-2 text-xs text-red-700">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            <span>{t("review.irreversibleWarning")}</span>
          </div>
        </div>

        <Separator />

        <DialogFooter className="flex items-center justify-end gap-2">
          <Button variant="outline" size="sm" onClick={handleClose}>
            {t("form.cancel")}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={handleReject}
            disabled={saving || reasonError}
            className="gap-1.5"
          >
            {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("review.rejectAndConfirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

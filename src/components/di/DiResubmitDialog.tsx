/**
 * DiResubmitDialog.tsx
 *
 * The requester answers the reviewer's question and sends the request back to review
 * (`resubmit_di`). The answer is kept on the request's timeline next to the question.
 */

import { Loader2, Send } from "lucide-react";
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
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useDiStore } from "@/stores/di-store";
import type { InterventionRequest } from "@shared/ipc-types";

const ANSWER_MAX = 2000;

interface DiResubmitDialogProps {
  di: InterventionRequest;
  /** The reviewer's question (reason label and note), shown above the answer. */
  question: { reason: string | null; note: string | null };
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function DiResubmitDialog({ di, question, open, onOpenChange }: DiResubmitDialogProps) {
  const { t } = useTranslation("di");
  const resubmit = useDiStore((s) => s.resubmit);
  const [answer, setAnswer] = useState("");
  const [touched, setTouched] = useState(false);
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!open) return;
    setAnswer("");
    setTouched(false);
  }, [open]);

  const missing = !answer.trim();

  const handleSubmit = async () => {
    setTouched(true);
    if (missing) return;
    setPending(true);
    try {
      await resubmit({
        di_id: di.id,
        actor_id: 0, // the backend uses the session user
        expected_row_version: di.row_version,
        response_note: answer.trim(),
      });
      onOpenChange(false);
    } catch {
      // The global IPC toast reports the reason; keep the dialog open to retry.
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{t("resubmit.title", { code: di.code })}</DialogTitle>
          <DialogDescription>{t("resubmit.description")}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-2">
          <section
            aria-label={t("resubmit.question")}
            className="space-y-1 rounded-lg border border-surface-border bg-surface-2 px-3 py-2"
          >
            <p className="text-xs font-semibold text-text-muted">{t("resubmit.question")}</p>
            {question.reason ? (
              <p className="text-sm font-medium text-text-primary">{question.reason}</p>
            ) : null}
            {question.note ? (
              <p className="whitespace-pre-wrap break-words text-sm text-text-primary">
                {question.note}
              </p>
            ) : null}
          </section>
          <div className="space-y-1.5">
            <Label htmlFor="di-resubmit-answer">
              {t("resubmit.answer")} <span aria-hidden>*</span>
            </Label>
            <Textarea
              id="di-resubmit-answer"
              rows={4}
              maxLength={ANSWER_MAX}
              value={answer}
              placeholder={t("resubmit.answerPlaceholder")}
              aria-invalid={touched && missing}
              aria-describedby={touched && missing ? "di-resubmit-answer-error" : undefined}
              onChange={(e) => setAnswer(e.target.value)}
              onBlur={() => setTouched(true)}
            />
            {touched && missing ? (
              <p id="di-resubmit-answer-error" className="text-xs text-tone-danger-fg">
                {t("resubmit.answerRequired")}
              </p>
            ) : null}
            <p className="text-xs text-text-muted">{t("resubmit.editHint")}</p>
          </div>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            disabled={pending}
            onClick={() => onOpenChange(false)}
          >
            {t("form.cancel")}
          </Button>
          <Button className="h-11 gap-1.5" disabled={pending} onClick={() => void handleSubmit()}>
            {pending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <Send className="h-4 w-4" aria-hidden />
            )}
            {t("resubmit.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

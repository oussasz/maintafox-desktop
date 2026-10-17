/**
 * DiScreenDialog.tsx
 *
 * Screening (tri) of a DI in review: the reviewer confirms or changes the urgency,
 * sets the classification and may leave a note. Used from the detail dialog and the
 * review queue so both paths record what the reviewer actually decided instead of
 * copying the requester's values.
 */

import { ClipboardCheck, Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { UrgencySelect } from "@/components/common/urgency/UrgencySelect";
import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import { useDiReviewStore } from "@/stores/di-review-store";
import type { InterventionRequest, ReferenceValue } from "@shared/ipc-types";

/**
 * Classification uses the failure-symptom list shared by requests, the OT close-out and RAMS
 * (`WORK.SYMPTOMS`, ISO 14224): the value the conversion carries onto the WO failure details.
 */
const CLASSIFICATION_DOMAIN = "WORK.SYMPTOMS";
const NONE = "__none";

interface DiScreenDialogProps {
  di: InterventionRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Called with the updated DI after a successful screening. */
  onScreened?: (updated: InterventionRequest) => void;
}

export function DiScreenDialog({ di, open, onOpenChange, onScreened }: DiScreenDialogProps) {
  const { t } = useTranslation("di");
  const screen = useDiReviewStore((s) => s.screen);
  const [urgency, setUrgency] = useState<string>("");
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const [classificationId, setClassificationId] = useState<string>(NONE);
  const [note, setNote] = useState("");
  const [classifications, setClassifications] = useState<ReferenceValue[]>([]);
  const [catalogState, setCatalogState] = useState<"loading" | "ready" | "error">("loading");
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!open || !di) return;
    setUrgency(di.validated_urgency ?? di.reported_urgency);
    const initial = di.classification_code_id ?? di.symptom_code_id;
    setClassificationId(initial != null ? String(initial) : NONE);
    setNote(di.reviewer_note ?? "");
  }, [open, di]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setCatalogState("loading");
    listPublishedReferenceValuesByDomainCode(CLASSIFICATION_DOMAIN)
      .then((values) => {
        if (cancelled) return;
        setClassifications(values.filter((v) => v.is_active));
        setCatalogState("ready");
      })
      .catch(() => {
        if (!cancelled) setCatalogState("error");
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const sortedClassifications = useMemo(
    () => [...classifications].sort((a, b) => a.label.localeCompare(b.label)),
    [classifications],
  );

  if (!di) return null;

  const urgencyChanged = urgency !== di.reported_urgency;

  const handleSubmit = async () => {
    setPending(true);
    try {
      const updated = await screen({
        di_id: di.id,
        actor_id: 0, // the backend uses the session user
        expected_row_version: di.row_version,
        validated_urgency: urgency,
        classification_code_id: classificationId === NONE ? null : Number(classificationId),
        reviewer_note: note.trim() || null,
      });
      onOpenChange(false);
      onScreened?.(updated);
    } catch {
      // The global IPC toast reports the reason; keep the dialog open to correct and retry.
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle>{t("screening.title", { code: di.code })}</DialogTitle>
          <DialogDescription>{t("screening.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label htmlFor="di-screen-urgency">{t("screening.validatedUrgency")}</Label>
            <UrgencySelect
              id="di-screen-urgency"
              value={urgency || null}
              onChange={setUrgency}
              className="h-11"
            />
            <p className="text-xs text-text-muted">
              {urgencyChanged
                ? t("screening.urgencyChanged", { reported: urgencyLabel(di.reported_urgency) })
                : t("screening.urgencyAsReported")}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="di-screen-classification">{t("screening.classification")}</Label>
            <Select
              value={classificationId}
              onValueChange={setClassificationId}
              disabled={catalogState !== "ready"}
            >
              <SelectTrigger id="di-screen-classification" className="h-11">
                <SelectValue
                  placeholder={
                    catalogState === "loading"
                      ? t("screening.classificationLoading")
                      : t("screening.classificationPlaceholder")
                  }
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{t("screening.classificationNone")}</SelectItem>
                {sortedClassifications.map((v) => (
                  <SelectItem key={v.id} value={String(v.id)}>
                    {v.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-text-muted">
              {catalogState === "error"
                ? t("screening.classificationUnavailable")
                : t("screening.classificationHelp")}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="di-screen-note">{t("screening.note")}</Label>
            <Textarea
              id="di-screen-note"
              rows={3}
              value={note}
              placeholder={t("screening.notePlaceholder")}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
        </div>

        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            disabled={pending}
            onClick={() => onOpenChange(false)}
          >
            {t("screening.cancel")}
          </Button>
          <Button className="h-11 gap-1.5" disabled={pending} onClick={() => void handleSubmit()}>
            {pending ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <ClipboardCheck className="h-4 w-4" aria-hidden />
            )}
            {t("screening.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

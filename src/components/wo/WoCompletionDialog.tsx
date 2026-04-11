/**
 * WoCompletionDialog.tsx
 *
 * Completion confirmation modal for a work order.
 * Pre-fills end date with now, accepts hours worked + completion report.
 * Displays pre-flight blocking errors returned by `complete_wo_mechanically`.
 *
 * Phase 2 – Sub-phase 05 – File 02 – Sprint S4.
 */

import { AlertTriangle, CheckCircle, Clock, FileText } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

// ── Helpers ──────────────────────────────────────────────────────────────────

function toDatetimeLocal(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const ERROR_ICONS: Record<string, typeof AlertTriangle> = {
  OPEN_LABOR: Clock,
  INCOMPLETE_TASKS: FileText,
  MISSING_PARTS: AlertTriangle,
  OPEN_DOWNTIME: AlertTriangle,
};

// ── Component ───────────────────────────────────────────────────────────────

interface WoCompletionDialogProps {
  wo: WorkOrder;
}

export function WoCompletionDialog({ wo }: WoCompletionDialogProps) {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const open = useWoStore((s) => s.showCompletionDialog);
  const errors = useWoStore((s) => s.completionErrors);
  const closeDialog = useWoStore((s) => s.closeCompletionDialog);
  const completeWorkOrder = useWoStore((s) => s.completeWorkOrder);
  const saving = useWoStore((s) => s.saving);

  const [endDate, setEndDate] = useState(() => toDatetimeLocal(new Date()));
  const [hoursWorked, setHoursWorked] = useState("");
  const [conclusion, setConclusion] = useState("");

  const actorId = info?.user_id ?? 0;

  const handleSubmit = useCallback(async () => {
    if (!actorId) return;
    await completeWorkOrder({
      wo_id: wo.id,
      actor_id: actorId,
      expected_row_version: wo.row_version,
      actual_end: endDate || null,
      actual_duration_hours: hoursWorked ? Number(hoursWorked) : null,
      conclusion: conclusion || null,
    });
  }, [wo, actorId, endDate, hoursWorked, conclusion, completeWorkOrder]);

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && closeDialog()}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <CheckCircle className="h-5 w-5 text-amber-600" />
            {t("completion.title")}
          </DialogTitle>
          <DialogDescription>
            <span className="font-mono text-xs">{wo.code}</span>
            {" — "}
            {wo.title}
          </DialogDescription>
        </DialogHeader>

        <Separator />

        <div className="space-y-4 py-2">
          {/* ── Pre-flight errors ─────────────────────────────────── */}
          {errors.length > 0 && (
            <div className="space-y-2">
              <h4 className="text-sm font-semibold text-destructive flex items-center gap-1.5">
                <AlertTriangle className="h-4 w-4" />
                {t("completion.blockers")}
              </h4>
              {errors.map((err) => {
                const Icon = ERROR_ICONS[err.code] ?? AlertTriangle;
                return (
                  <div
                    key={err.code}
                    className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 p-2.5 text-sm"
                  >
                    <Icon className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
                    <div>
                      <Badge variant="outline" className="text-[10px] mr-1.5">
                        {err.code}
                      </Badge>
                      {err.message}
                    </div>
                  </div>
                );
              })}
              <Separator />
            </div>
          )}

          {/* ── End date/time ─────────────────────────────────────── */}
          <div className="space-y-1.5">
            <Label className="text-xs">{t("completion.endDate")}</Label>
            <Input
              type="datetime-local"
              value={endDate}
              onChange={(e) => setEndDate(e.target.value)}
              className="h-8 text-sm"
            />
          </div>

          {/* ── Hours worked ──────────────────────────────────────── */}
          <div className="space-y-1.5">
            <Label className="text-xs">{t("completion.hoursWorked")}</Label>
            <Input
              type="number"
              min={0}
              step={0.5}
              value={hoursWorked}
              onChange={(e) => setHoursWorked(e.target.value)}
              placeholder={t("completion.hoursPlaceholder")}
              className="h-8 text-sm"
            />
          </div>

          {/* ── Observations / report ─────────────────────────────── */}
          <div className="space-y-1.5">
            <Label className="text-xs">{t("completion.report")}</Label>
            <Textarea
              value={conclusion}
              onChange={(e) => setConclusion(e.target.value)}
              placeholder={t("completion.reportPlaceholder")}
              rows={4}
              className="text-sm resize-none"
            />
          </div>
        </div>

        <Separator />

        {/* ── Footer actions ──────────────────────────────────────── */}
        <div className="flex items-center justify-end gap-2 pt-1">
          <Button variant="outline" size="sm" onClick={closeDialog}>
            {t("form.cancel")}
          </Button>
          <Button
            size="sm"
            onClick={() => void handleSubmit()}
            disabled={saving || !endDate}
            className="bg-amber-600 hover:bg-amber-700 text-white gap-1.5"
          >
            <CheckCircle className="h-3.5 w-3.5" />
            {t("completion.submit")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

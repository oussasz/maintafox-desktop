/**
 * DiDetailDialog.tsx
 *
 * Floating detail window for a single intervention request.
 * Renders as a centered modal dialog (not a side panel) so the underlying
 * Kanban / list view stays fully visible and interactive when closed.
 *
 * UX pattern: "Entity Detail Dialog" — the unified desktop detail view
 * pattern described in PRD §UX-DW-001. All entity detail views (DI, WO,
 * Equipment, etc.) should follow this pattern:
 *   • Centered modal overlay with `max-w-3xl` / `max-h-[85vh]`
 *   • Header: entity code + title + status badge + close button
 *   • Body: scrollable info section + tabbed sub-panels
 *   • Footer: contextual action buttons (approve, reject, etc.)
 *
 * Phase 2 – Sub-phase 04 – Sprint S4.
 */

import { Calendar, Check, ClipboardCheck, Printer, RotateCcw, Shield, User, X } from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { DiDetailPanel } from "@/components/di/DiDetailPanel";
import { printDiFiche } from "@/components/di/DiPrintFiche";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { DiTransitionRow, InterventionRequest } from "@shared/ipc-types";

// ── Status → badge style mapping ────────────────────────────────────────────

const STATUS_STYLE: Record<string, string> = {
  submitted: "bg-blue-100 text-blue-800",
  pending_review: "bg-amber-100 text-amber-800",
  returned_for_clarification: "bg-orange-100 text-orange-800",
  rejected: "bg-red-100 text-red-700",
  screened: "bg-sky-100 text-sky-800",
  awaiting_approval: "bg-yellow-100 text-yellow-800",
  approved_for_planning: "bg-green-100 text-green-800",
  deferred: "bg-gray-100 text-gray-600",
  converted_to_work_order: "bg-emerald-100 text-emerald-800",
  closed_as_non_executable: "bg-slate-100 text-slate-600",
  archived: "bg-neutral-100 text-neutral-500",
};

const URGENCY_STYLE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

type DiStatusKey =
  | "new"
  | "inReview"
  | "approved"
  | "rejected"
  | "inProgress"
  | "resolved"
  | "closed"
  | "cancelled";

function statusToI18nKey(s: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    submitted: "new",
    pending_review: "inReview",
    returned_for_clarification: "inReview",
    rejected: "rejected",
    screened: "inReview",
    awaiting_approval: "inReview",
    approved_for_planning: "approved",
    deferred: "inReview",
    converted_to_work_order: "inProgress",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[s] ?? "new";
}

// ── Props ───────────────────────────────────────────────────────────────────

interface DiDetailDialogProps {
  di: InterventionRequest | null;
  /** State transition log from `get_di` (formal audit). */
  transitions: DiTransitionRow[];
  open: boolean;
  onClose: () => void;
}

// ── Statuses that can be acted on ───────────────────────────────────────────

const SCREENABLE = new Set(["pending_review", "returned_for_clarification"]);
const APPROVABLE = new Set(["awaiting_approval"]);
const REVIEWABLE = new Set([
  "submitted",
  "pending_review",
  "returned_for_clarification",
  "screened",
  "awaiting_approval",
]);

// ── Component ───────────────────────────────────────────────────────────────

export function DiDetailDialog({ di, transitions, open, onClose }: DiDetailDialogProps) {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const { can } = usePermissions();
  const { info } = useSession();
  const triageSubmittedDi = useDiStore((s) => s.triageSubmittedDi);
  const triageSaving = useDiStore((s) => s.saving);
  const openApproval = useDiReviewStore((s) => s.openApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const openReturn = useDiReviewStore((s) => s.openReturn);
  const screen = useDiReviewStore((s) => s.screen);
  const [screenError, setScreenError] = useState<string | null>(null);
  const [triageError, setTriageError] = useState<string | null>(null);

  const handleScreen = useCallback(async () => {
    if (!di) return;
    setScreenError(null);
    try {
      const updated = await screen({
        di_id: di.id,
        actor_id: 0,
        expected_row_version: di.row_version,
        validated_urgency: di.reported_urgency,
        classification_code_id: di.classification_code_id ?? di.symptom_code_id ?? null,
        reviewer_note: null,
      });
      if (updated.status === "awaiting_approval") {
        onClose();
        openApproval(updated);
      }
    } catch (err) {
      setScreenError(String(err));
    }
  }, [di, screen, onClose, openApproval]);

  const handleApprove = useCallback(() => {
    if (!di) return;
    onClose();
    openApproval(di);
  }, [di, onClose, openApproval]);

  const handleReject = useCallback(() => {
    if (!di) return;
    onClose();
    openRejection(di);
  }, [di, onClose, openRejection]);

  const handleReturn = useCallback(() => {
    if (!di) return;
    onClose();
    openReturn(di);
  }, [di, onClose, openReturn]);

  const handleTriageAccept = useCallback(async () => {
    if (!di) return;
    setTriageError(null);
    try {
      await triageSubmittedDi({ di_id: di.id, expected_row_version: di.row_version });
      window.dispatchEvent(new Event("mf:di-triage-refresh"));
      window.dispatchEvent(new Event("mf:dashboard-kpis-refresh"));
    } catch (e) {
      setTriageError(e instanceof Error ? e.message : String(e));
    }
  }, [di, triageSubmittedDi]);

  if (!di) return null;

  const statusKey = statusToI18nKey(di.status);
  const canUseReviewQueueActions = can("di.review") && REVIEWABLE.has(di.status);
  const canRunScreen = (can("di.screen") || can("di.review")) && SCREENABLE.has(di.status);
  const canTriageToReviewQueue =
    di.status === "submitted" &&
    (can("di.screen") || can("di.review")) &&
    info?.user_id != null &&
    info.user_id !== di.submitter_id;
  const canUploadAttachment =
    info != null &&
    ((info.user_id === di.submitter_id && can("di.create.own")) || can("di.review"));
  const canDeleteAttachment = can("di.admin");

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent
        className="max-w-3xl max-h-[85vh] flex flex-col p-0 gap-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        {/* ── Header ──────────────────────────────────────────────────── */}
        <DialogHeader className="px-6 pt-5 pb-3">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1 min-w-0">
              <DialogTitle className="text-lg font-bold flex items-center gap-2">
                <span className="text-muted-foreground font-mono text-base">{di.code}</span>
                {di.is_modified && (
                  <Badge className="bg-amber-100 text-amber-800 border-0 text-[10px] px-1.5 py-0">
                    {t("review.modified")}
                  </Badge>
                )}
                <span className="truncate">{di.title}</span>
              </DialogTitle>
              <DialogDescription className="text-sm text-muted-foreground line-clamp-2">
                {di.description}
              </DialogDescription>
            </div>
            <div className="flex items-center gap-1.5 shrink-0 pt-0.5">
              <Badge
                variant="outline"
                className={`text-xs border-0 ${STATUS_STYLE[di.status] ?? "bg-gray-100"}`}
              >
                {t(`status.${statusKey}` as const)}
              </Badge>
              {di.safety_flag && (
                <Badge variant="destructive" className="text-xs gap-1">
                  <Shield className="h-3 w-3" />
                  {t("detail.safety")}
                </Badge>
              )}
            </div>
          </div>
        </DialogHeader>

        <Separator />

        {/* ── Scrollable body ─────────────────────────────────────────── */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Info grid */}
          <Card>
            <CardContent className="p-3 grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-2 text-xs">
              <InfoRow
                icon={<Calendar className="h-3 w-3" />}
                label={t("detail.fields.reportedAt")}
                value={formatDiDate(di.submitted_at, dateLocale, {
                  day: "2-digit",
                  month: "2-digit",
                  year: "numeric",
                })}
              />
              <InfoRow
                icon={<User className="h-3 w-3" />}
                label={t("detail.fields.reportedBy")}
                value={`#${di.submitter_id}`}
              />
              <InfoRow
                label={t("detail.fields.priority")}
                value={
                  <Badge
                    variant="outline"
                    className={`text-[10px] border-0 ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
                  >
                    {t(`priority.${di.reported_urgency}`)}
                  </Badge>
                }
              />
              <InfoRow label={t("detail.fields.origin")} value={di.origin_type} />
              <InfoRow label={t("detail.fields.asset")} value={`#${di.asset_id}`} />
              <InfoRow label={t("detail.fields.orgNode")} value={`#${di.org_node_id}`} />
              {di.production_impact && (
                <InfoRow label={t("detail.fields.impact")} value={t("detail.fields.production")} />
              )}
            </CardContent>
          </Card>

          {/* Tabs: attachments + audit */}
          <DiDetailPanel
            di={di}
            transitions={transitions}
            canUploadAttachment={canUploadAttachment}
            canDeleteAttachment={canDeleteAttachment}
          />
        </div>

        {/* ── Footer ──────────────────────────────────────────────────── */}
        <Separator />
        <div className="flex items-center justify-between gap-2 px-6 py-3">
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => printDiFiche(di)}
              className="gap-1.5"
            >
              <Printer className="h-3.5 w-3.5" />
              {t("review.print")}
            </Button>
          </div>
          <div className="flex flex-col items-end gap-1 sm:flex-row sm:items-center sm:gap-2">
            {canTriageToReviewQueue && (
              <Button
                size="sm"
                className="gap-1.5"
                disabled={triageSaving}
                onClick={() => void handleTriageAccept()}
              >
                <ClipboardCheck className="h-3.5 w-3.5" />
                {t("triage.acceptForReview")}
              </Button>
            )}
            {canRunScreen && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-blue-600 hover:text-blue-700 hover:bg-blue-50"
                onClick={handleScreen}
                title={t("review.screenAction")}
              >
                <ClipboardCheck className="h-3.5 w-3.5" />
                {t("review.screenAction")}
              </Button>
            )}
            {canUseReviewQueueActions && APPROVABLE.has(di.status) && (
              <Button
                size="sm"
                className="gap-1.5 bg-green-600 hover:bg-green-700 text-white"
                onClick={handleApprove}
                title={t("action.approve")}
              >
                <Check className="h-3.5 w-3.5" />
                {t("action.approve")}
              </Button>
            )}
            {canUseReviewQueueActions && di.status !== "submitted" && (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5 text-amber-600 hover:text-amber-700 hover:bg-amber-50"
                  onClick={handleReturn}
                  title={t("review.returnAction")}
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                  {t("review.returnAction")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-1.5 text-red-600 hover:text-red-700 hover:bg-red-50"
                  onClick={handleReject}
                  title={t("action.reject")}
                >
                  <X className="h-3.5 w-3.5" />
                  {t("action.reject")}
                </Button>
              </>
            )}
            <Button variant="outline" size="sm" onClick={onClose} className="gap-1.5">
              {t("detail.close")}
            </Button>
          </div>
        </div>

        {/* Screen / triage error */}
        {(triageError ?? screenError) && (
          <div className="px-6 pb-3">
            <div
              role="alert"
              className="rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {triageError ?? screenError}
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function InfoRow({ icon, label, value }: { icon?: ReactNode; label: string; value: ReactNode }) {
  return (
    <div className="flex items-center gap-1.5">
      {icon}
      <span className="text-muted-foreground">{label}:</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}

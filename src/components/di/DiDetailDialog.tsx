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

import {
  Archive,
  Calendar,
  Check,
  ClipboardCheck,
  Printer,
  RotateCcw,
  Shield,
  User,
  X,
} from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DiDetailPanel } from "@/components/di/DiDetailPanel";
import { printDiFiche } from "@/components/di/DiPrintFiche";
import { DiSlaStatusBadge } from "@/components/di/DiSlaStatusBadge";
import { useDiReferenceLabels } from "@/components/di/di-reference-labels";
import { dispositionLabelKey } from "@/components/di/disposition-meta";
import {
  DI_STATUS_STYLE,
  TERMINAL_DI_STATES,
  diStatusToI18nKey,
} from "@/components/di/status-meta";
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
import {
  formatAssetLabel,
  formatOrgNodeLabel,
  formatOrDash,
  formatPersonLabel,
} from "@/lib/display";
import { getSlaStatus } from "@/services/di-conversion-service";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { DiSlaStatus, DiTransitionRow, InterventionRequest } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const URGENCY_STYLE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

// ── Props ───────────────────────────────────────────────────────────────────

interface DiDetailDialogProps {
  di: InterventionRequest | null;
  /** State transition log from `get_di` (formal audit). */
  transitions: DiTransitionRow[];
  open: boolean;
  onClose: () => void;
}

// ── Statuses that can be acted on ───────────────────────────────────────────

const SCREENABLE = new Set(["in_review"]);
const APPROVABLE = new Set(["awaiting_approval"]);
const CLOSEABLE_REVIEW = new Set(["in_review", "awaiting_approval"]);
const CLOSEABLE_APPROVE = new Set(["approved"]);
const RETURNABLE = new Set(["in_review"]);
const DEFERRABLE = new Set(["in_review", "awaiting_approval", "approved"]);

// ── Component ───────────────────────────────────────────────────────────────

export function DiDetailDialog({ di, transitions, open, onClose }: DiDetailDialogProps) {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const { originLabel, requestTypeLabel, symptomLabel } = useDiReferenceLabels({
    includeSymptoms: true,
  });
  const { can } = usePermissions();
  const { info } = useSession();
  const triageSubmittedDi = useDiStore((s) => s.triageSubmittedDi);
  const triageSaving = useDiStore((s) => s.saving);
  const openApproval = useDiReviewStore((s) => s.openApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const openReturn = useDiReviewStore((s) => s.openReturn);
  const screen = useDiReviewStore((s) => s.screen);
  const defer = useDiReviewStore((s) => s.defer);
  const cancelOwn = useDiReviewStore((s) => s.cancelOwn);
  const archiveDi = useDiReviewStore((s) => s.archive);
  const [screenError, setScreenError] = useState<string | null>(null);
  const [triageError, setTriageError] = useState<string | null>(null);
  const [slaStatus, setSlaStatus] = useState<DiSlaStatus | null>(null);

  useEffect(() => {
    if (!open || !di) {
      setSlaStatus(null);
      return;
    }
    let cancelled = false;
    void getSlaStatus(di.id)
      .then((status) => {
        if (!cancelled) setSlaStatus(status);
      })
      .catch(() => {
        if (!cancelled) setSlaStatus(null);
      });
    return () => {
      cancelled = true;
    };
  }, [open, di]);

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

  const handleCloseRequest = useCallback(() => {
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

  const handleCancelOwn = useCallback(async () => {
    if (!di || info?.user_id == null) return;
    setScreenError(null);
    try {
      await cancelOwn({
        di_id: di.id,
        actor_id: info.user_id,
        expected_row_version: di.row_version,
        notes: null,
      });
      window.dispatchEvent(new Event("mf:di-triage-refresh"));
      window.dispatchEvent(new Event("mf:dashboard-kpis-refresh"));
    } catch (e) {
      setScreenError(e instanceof Error ? e.message : String(e));
    }
  }, [cancelOwn, di, info]);

  const handleDefer = useCallback(async () => {
    if (!di || info?.user_id == null) return;
    setScreenError(null);
    const deferredUntil = new Date();
    deferredUntil.setDate(deferredUntil.getDate() + 30);
    try {
      await defer({
        di_id: di.id,
        actor_id: info.user_id,
        expected_row_version: di.row_version,
        deferred_until: deferredUntil.toISOString(),
        reason_code: "operational",
        notes: null,
      });
      window.dispatchEvent(new Event("mf:di-triage-refresh"));
      window.dispatchEvent(new Event("mf:dashboard-kpis-refresh"));
    } catch (e) {
      setScreenError(e instanceof Error ? e.message : String(e));
    }
  }, [defer, di, info]);

  const handleArchive = useCallback(async () => {
    if (!di) return;
    setScreenError(null);
    try {
      await archiveDi(di.id, di.row_version, null);
      window.dispatchEvent(new Event("mf:di-triage-refresh"));
      window.dispatchEvent(new Event("mf:dashboard-kpis-refresh"));
    } catch (e) {
      setScreenError(e instanceof Error ? e.message : String(e));
    }
  }, [archiveDi, di]);

  if (!di) return null;

  const statusKey = diStatusToI18nKey(di.status);
  const canRunScreen = (can(P.DI_SCREEN) || can(P.DI_REVIEW)) && SCREENABLE.has(di.status);
  const canReturnForClarification = can(P.DI_REVIEW) && RETURNABLE.has(di.status);
  const canCloseInReview = can(P.DI_REVIEW) && CLOSEABLE_REVIEW.has(di.status);
  const canCloseAfterApprove = can(P.DI_APPROVE) && CLOSEABLE_APPROVE.has(di.status);
  const canApproveForPlanning = can(P.DI_APPROVE) && APPROVABLE.has(di.status);
  const canDefer = can(P.DI_APPROVE) && DEFERRABLE.has(di.status);
  const canConvert = can(P.DI_CONVERT) && di.status === "approved" && di.converted_to_wo_id == null;
  const canArchive =
    (can(P.DI_APPROVE) || can(P.DI_ADMIN)) && di.status === "closed" && di.archived_at == null;
  const canCancelOwn =
    (di.status === "submitted" || di.status === "returned_for_clarification") &&
    info?.user_id != null &&
    info.user_id === di.submitter_id &&
    can(P.DI_CREATE_OWN);
  const canTriageToReviewQueue =
    di.status === "submitted" && (can(P.DI_SCREEN) || can(P.DI_REVIEW)) && info?.user_id != null;
  const canResubmitAsAuthor =
    di.status === "returned_for_clarification" &&
    info?.user_id != null &&
    info.user_id === di.submitter_id &&
    can(P.DI_CREATE_OWN);
  const canUploadAttachment =
    info != null &&
    !TERMINAL_DI_STATES.has(di.status) &&
    ((info.user_id === di.submitter_id && can(P.DI_CREATE_OWN)) || can(P.DI_REVIEW));
  const canDeleteAttachment = can(P.DI_ADMIN) && !TERMINAL_DI_STATES.has(di.status);

  const dispositionKey = dispositionLabelKey(di.disposition_code);
  const dispositionText = dispositionKey ? t(dispositionKey as "disposition.other") : null;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent
        className="w-full max-w-5xl max-h-[90vh] flex flex-col p-0 gap-0"
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
                className={`text-xs border-0 ${DI_STATUS_STYLE[di.status] ?? "bg-gray-100"}`}
              >
                {t(`status.${statusKey}` as const)}
              </Badge>
              <LinkedEntityBadge
                entity="work_order"
                code={di.converted_to_wo_code}
                entityId={di.converted_to_wo_id}
                title={di.converted_to_wo_title}
                className="text-xs"
              />
              {di.status === "closed" && dispositionText && (
                <Badge variant="outline" className="text-xs border-0 bg-slate-100 text-slate-700">
                  {dispositionText}
                </Badge>
              )}
              {di.status === "closed" && (
                <LinkedEntityBadge
                  entity="di"
                  code={di.related_di_code}
                  entityId={di.related_di_id}
                  title={null}
                  className="text-xs"
                />
              )}
              <DiSlaStatusBadge status={slaStatus?.status ?? null} className="text-xs" />
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
                value={formatPersonLabel(di.submitter_display_name)}
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
              <InfoRow label={t("detail.fields.origin")} value={originLabel(di.origin_type)} />
              <InfoRow
                label={t("detail.fields.requestType")}
                value={requestTypeLabel(di.request_type)}
              />
              {di.symptom_code_id != null && (
                <InfoRow
                  label={t("detail.fields.symptom")}
                  value={symptomLabel(di.symptom_code_id)}
                />
              )}
              <InfoRow
                label={t("detail.fields.asset")}
                value={formatAssetLabel(di.asset_code, di.asset_label)}
              />
              <InfoRow
                label={t("detail.fields.orgNode")}
                value={formatOrgNodeLabel(di.org_node_code, di.org_node_label)}
              />
              {di.production_impact && (
                <InfoRow label={t("detail.fields.impact")} value={t("detail.fields.production")} />
              )}
              {slaStatus?.sla_deadline && (
                <InfoRow
                  label={t("detail.fields.slaResponseDeadline")}
                  value={formatDiDate(slaStatus.sla_deadline, dateLocale)}
                />
              )}
              {slaStatus?.resolution_deadline && (
                <InfoRow
                  label={t("detail.fields.slaResolutionDeadline")}
                  value={formatDiDate(slaStatus.resolution_deadline, dateLocale)}
                />
              )}
              {slaStatus?.response_elapsed_hours != null && (
                <InfoRow
                  label={t("detail.fields.slaResponseElapsed")}
                  value={t("detail.fields.hoursValue", {
                    hours: Math.round(slaStatus.response_elapsed_hours * 10) / 10,
                  })}
                />
              )}
              {slaStatus?.resolution_elapsed_hours != null && (
                <InfoRow
                  label={t("detail.fields.slaResolutionElapsed")}
                  value={t("detail.fields.hoursValue", {
                    hours: Math.round(slaStatus.resolution_elapsed_hours * 10) / 10,
                  })}
                />
              )}
              {slaStatus?.response_remaining_hours != null && (
                <InfoRow
                  label={t("detail.fields.slaResponseRemaining")}
                  value={t("detail.fields.hoursValue", {
                    hours: Math.round(slaStatus.response_remaining_hours * 10) / 10,
                  })}
                />
              )}
              {slaStatus?.resolution_remaining_hours != null && (
                <InfoRow
                  label={t("detail.fields.slaResolutionRemaining")}
                  value={t("detail.fields.hoursValue", {
                    hours: Math.round(slaStatus.resolution_remaining_hours * 10) / 10,
                  })}
                />
              )}
              {slaStatus && !slaStatus.status && (
                <InfoRow label={t("detail.fields.slaStatus")} value={formatOrDash(null)} />
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
              onClick={() => {
                void printDiFiche(di);
              }}
              className="gap-1.5"
            >
              <Printer className="h-3.5 w-3.5" />
              {t("action.print")}
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
            {canResubmitAsAuthor && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-sky-700 hover:text-sky-800 hover:bg-sky-50"
                disabled={triageSaving}
                onClick={() => void handleTriageAccept()}
              >
                <RotateCcw className="h-3.5 w-3.5" />
                {t("action.resubmit")}
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
            {canApproveForPlanning && (
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
            {canDefer && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => void handleDefer()}
              >
                <Calendar className="h-3.5 w-3.5" />
                {t("action.defer")}
              </Button>
            )}
            {canConvert && (
              <Button variant="outline" size="sm" className="gap-1.5" onClick={handleApprove}>
                <Check className="h-3.5 w-3.5" />
                {t("action.convertToWO")}
              </Button>
            )}
            {canCloseAfterApprove && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-slate-700 hover:text-slate-800 hover:bg-slate-50"
                onClick={handleCloseRequest}
              >
                <X className="h-3.5 w-3.5" />
                {t("action.closeRequest")}
              </Button>
            )}
            {canArchive && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => void handleArchive()}
              >
                <Archive className="h-3.5 w-3.5" />
                {t("action.archive")}
              </Button>
            )}
            {canCancelOwn && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-red-600 hover:text-red-700 hover:bg-red-50"
                onClick={() => void handleCancelOwn()}
              >
                <X className="h-3.5 w-3.5" />
                {t("action.cancelOwn")}
              </Button>
            )}
            {(canReturnForClarification || canCloseInReview) && (
              <>
                {canReturnForClarification && (
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
                )}
                {canCloseInReview && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="gap-1.5 text-red-600 hover:text-red-700 hover:bg-red-50"
                    onClick={handleCloseRequest}
                    title={t("action.closeRequest")}
                  >
                    <X className="h-3.5 w-3.5" />
                    {t("action.closeRequest")}
                  </Button>
                )}
              </>
            )}
            <Button variant="outline" size="sm" onClick={onClose} className="gap-1.5">
              <X className="h-3.5 w-3.5" />
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

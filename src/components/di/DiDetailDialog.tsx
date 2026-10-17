/**
 * DiDetailDialog.tsx
 *
 * Floating detail window for a single intervention request (UX-DW-001 entity detail dialog):
 * header with code, title, status, SLA and links; what happens next; the reviewer's question
 * and the requester's answer; possible duplicates on the same asset; info grid; tabs
 * (attachments, timeline); footer with the decisions the user may take on this request.
 */

import {
  Archive,
  Calendar,
  CalendarClock,
  Check,
  ClipboardCheck,
  Copy,
  MessageSquareReply,
  Pencil,
  Printer,
  RotateCcw,
  Shield,
  User,
  X,
} from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge, roundTitle } from "@/components/common/LinkedEntityBadge";
import { UrgencyBadge } from "@/components/common/urgency/UrgencyBadge";
import { DiDeferDialog } from "@/components/di/DiDeferDialog";
import { DiDetailPanel } from "@/components/di/DiDetailPanel";
import { DiNextStepLabel } from "@/components/di/DiNextStepLabel";
import { printDiFiche } from "@/components/di/DiPrintFiche";
import { DiResubmitDialog } from "@/components/di/DiResubmitDialog";
import { DiScreenDialog } from "@/components/di/DiScreenDialog";
import { DiSlaStatusBadge } from "@/components/di/DiSlaStatusBadge";
import { diActions } from "@/components/di/di-actions";
import {
  useDiDecisionReasonLabels,
  useDiReferenceLabels,
} from "@/components/di/di-reference-labels";
import { dispositionLabelKey } from "@/components/di/disposition-meta";
import { TERMINAL_DI_STATES, diStatusToI18nKey } from "@/components/di/status-meta";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { StatusChip } from "@/components/ui/status-chip";
import { DI_STATUS_TONE, mfTone, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { describeAppError } from "@/lib/app-error";
import { formatAssetLabel, formatOrgNodeLabel, formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { getSlaStatus } from "@/services/di-conversion-service";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type {
  DiSlaStatus,
  DiSummaryRow,
  DiTransitionRow,
  InterventionRequest,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Props ───────────────────────────────────────────────────────────────────

interface DiDetailDialogProps {
  di: InterventionRequest | null;
  /** State transition log from `get_di` (formal audit). */
  transitions: DiTransitionRow[];
  /** Recent requests on the same asset with the same symptom (`get_di`). */
  similar?: DiSummaryRow[];
  open: boolean;
  onClose: () => void;
}

/** Last entry of the transition log with the given action. */
function lastTransition(rows: DiTransitionRow[], action: string): DiTransitionRow | undefined {
  for (let i = rows.length - 1; i >= 0; i -= 1) {
    if (rows[i]?.action === action) return rows[i];
  }
  return undefined;
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiDetailDialog({
  di,
  transitions,
  similar = [],
  open,
  onClose,
}: DiDetailDialogProps) {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const { originLabel, requestTypeLabel, symptomLabel } = useDiReferenceLabels({
    includeSymptoms: true,
  });
  const { returnReasonLabel } = useDiDecisionReasonLabels();
  const { can } = usePermissions();
  const { info } = useSession();
  const triageSubmittedDi = useDiStore((s) => s.triageSubmittedDi);
  const openDi = useDiStore((s) => s.openDi);
  const openCreateForm = useDiStore((s) => s.openCreateForm);
  const triageSaving = useDiStore((s) => s.saving);
  const openApproval = useDiReviewStore((s) => s.openApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const openReturn = useDiReviewStore((s) => s.openReturn);
  const cancelOwn = useDiReviewStore((s) => s.cancelOwn);
  const archiveDi = useDiReviewStore((s) => s.archive);
  const reactivate = useDiReviewStore((s) => s.reactivate);
  const [actionError, setActionError] = useState<string | null>(null);
  const [screenOpen, setScreenOpen] = useState(false);
  const [deferOpen, setDeferOpen] = useState(false);
  const [resubmitOpen, setResubmitOpen] = useState(false);
  const [confirmAction, setConfirmAction] = useState<"cancelOwn" | "archive" | "reactivate" | null>(
    null,
  );
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

  useEffect(() => {
    if (!open) setActionError(null);
  }, [open]);

  const refreshQueues = useCallback(() => {
    window.dispatchEvent(new Event("mf:di-triage-refresh"));
    window.dispatchEvent(new Event("mf:dashboard-kpis-refresh"));
  }, []);

  const handleScreened = useCallback(
    (updated: InterventionRequest) => {
      refreshQueues();
      if (updated.status === "awaiting_approval" && can(P.DI_APPROVE)) {
        onClose();
        openApproval(updated);
      } else {
        void openDi(updated.id);
      }
    },
    [can, onClose, openApproval, openDi, refreshQueues],
  );

  const handOff = useCallback(
    (open: (di: InterventionRequest) => void) => {
      if (!di) return;
      onClose();
      open(di);
    },
    [di, onClose],
  );

  const handleTriageAccept = useCallback(async () => {
    if (!di) return;
    setActionError(null);
    try {
      await triageSubmittedDi({ di_id: di.id, expected_row_version: di.row_version });
      refreshQueues();
    } catch (e) {
      setActionError(describeAppError(e).title);
    }
  }, [di, triageSubmittedDi, refreshQueues]);

  /** Runs inside ConfirmDialog: a rejection keeps it open and the global toast explains why. */
  const handleCancelOwn = useCallback(async () => {
    if (!di || info?.user_id == null) return;
    await cancelOwn({
      di_id: di.id,
      actor_id: info.user_id,
      expected_row_version: di.row_version,
      notes: null,
    });
    refreshQueues();
    void openDi(di.id);
  }, [cancelOwn, di, info, refreshQueues, openDi]);

  const handleArchive = useCallback(async () => {
    if (!di) return;
    await archiveDi(di.id, di.row_version, null);
    refreshQueues();
    void openDi(di.id);
  }, [archiveDi, di, refreshQueues, openDi]);

  const handleReactivate = useCallback(async () => {
    if (!di) return;
    await reactivate({
      di_id: di.id,
      actor_id: 0, // the backend uses the session user
      expected_row_version: di.row_version,
      notes: null,
    });
    refreshQueues();
    void openDi(di.id);
  }, [reactivate, di, refreshQueues, openDi]);

  const handlePrint = useCallback(() => {
    if (!di) return;
    setActionError(null);
    printDiFiche(di).catch((err: unknown) => setActionError(describeAppError(err).title));
  }, [di]);

  const returned = useMemo(
    () => lastTransition(transitions, "return_for_clarification"),
    [transitions],
  );
  const answer = useMemo(() => lastTransition(transitions, "re_submitted"), [transitions]);
  const duplicates = useMemo(
    () => (di ? similar.filter((s) => s.id !== di.id && s.status !== "closed") : []),
    [similar, di],
  );

  if (!di) return null;

  const actions = diActions(di, info?.user_id, can);
  const canUploadAttachment =
    info != null &&
    !TERMINAL_DI_STATES.has(di.status) &&
    ((info.user_id === di.submitter_id && can(P.DI_CREATE_OWN)) || can(P.DI_REVIEW));
  const canDeleteAttachment = can(P.DI_ADMIN) && !TERMINAL_DI_STATES.has(di.status);

  const dispositionKey = dispositionLabelKey(di.disposition_code);
  const dispositionText = dispositionKey ? t(dispositionKey as "disposition.other") : null;
  const question = {
    reason: returned?.reason_code ? returnReasonLabel(returned.reason_code) : null,
    note: di.reviewer_note ?? returned?.notes ?? null,
  };
  const shortDate = (iso: string | null | undefined) =>
    formatDiDate(iso, dateLocale, { day: "2-digit", month: "2-digit", year: "numeric" });

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
                <span className="truncate">{di.title}</span>
              </DialogTitle>
              <DialogDescription className="text-sm text-muted-foreground line-clamp-2">
                {di.description}
              </DialogDescription>
            </div>
            <div className="flex flex-wrap items-center justify-end gap-1.5 shrink-0 pt-0.5">
              <StatusChip
                label={t(`status.${diStatusToI18nKey(di.status)}` as const)}
                tone={toneOf(DI_STATUS_TONE, di.status)}
                dot
                size="md"
              />
              <LinkedEntityBadge
                entity="work_order"
                code={di.converted_to_wo_code}
                entityId={di.converted_to_wo_id}
                title={di.converted_to_wo_title}
                className="text-xs"
              />
              {di.status === "closed" && dispositionText && (
                <StatusChip label={dispositionText} tone="neutral" size="md" />
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
              <DiSlaStatusBadge
                status={slaStatus?.status ?? di.sla_state ?? null}
                requestStatus={di.status}
              />
              {di.safety_flag && (
                <StatusChip label={t("detail.safety")} tone="danger" icon={Shield} size="md" />
              )}
            </div>
          </div>
        </DialogHeader>

        <Separator />

        {/* ── Scrollable body ─────────────────────────────────────────── */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* What happens next */}
          <section
            aria-labelledby="di-detail-next-step"
            className="flex flex-wrap items-center gap-2 rounded-lg border border-surface-border bg-surface-2 px-3 py-2"
          >
            <h3 id="di-detail-next-step" className="text-xs font-semibold text-text-muted">
              {t("detail.nextStep")}
            </h3>
            <DiNextStepLabel di={di} className="text-sm" />
          </section>

          {/* The reviewer's question, while the requester has to answer */}
          {di.status === "returned_for_clarification" && (question.reason || question.note) ? (
            <section
              aria-labelledby="di-detail-question"
              className={cn("space-y-1 rounded-lg border px-3 py-2", mfTone.attention.panel)}
            >
              <h3 id="di-detail-question" className="text-xs font-semibold text-text-primary">
                {t("detail.reviewerQuestion")}
              </h3>
              {question.reason ? (
                <p className="text-sm font-medium text-text-primary">{question.reason}</p>
              ) : null}
              {question.note ? (
                <p className="whitespace-pre-wrap break-words text-sm text-text-primary">
                  {question.note}
                </p>
              ) : null}
            </section>
          ) : null}

          {/* The requester's answer, for the reviewer */}
          {answer?.notes && di.status !== "returned_for_clarification" ? (
            <section aria-labelledby="di-detail-answer" className="space-y-1">
              <h3 id="di-detail-answer" className="text-xs font-semibold text-text-muted">
                {t("detail.requesterAnswer", {
                  name: formatPersonLabel(answer.actor_display_name ?? null),
                  date: shortDate(answer.acted_at),
                })}
              </h3>
              <p className="whitespace-pre-wrap break-words text-sm text-text-primary">
                {answer.notes}
              </p>
            </section>
          ) : null}

          {/* Possible duplicates on the same asset */}
          {duplicates.length > 0 && !TERMINAL_DI_STATES.has(di.status) ? (
            <section
              aria-labelledby="di-detail-duplicates"
              className="space-y-2 rounded-lg border border-surface-border px-3 py-2"
            >
              <h3
                id="di-detail-duplicates"
                className="flex items-center gap-1.5 text-xs font-semibold text-text-primary"
              >
                <Copy className="h-3.5 w-3.5" aria-hidden />
                {t("detail.possibleDuplicates", { count: duplicates.length })}
              </h3>
              <ul className="space-y-1">
                {duplicates.map((s) => (
                  <li key={s.id}>
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 rounded px-1 py-1 text-left text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      onClick={() => void openDi(s.id)}
                    >
                      <span className="font-mono text-text-muted">{s.code}</span>
                      <span className="min-w-0 flex-1 truncate">{s.title}</span>
                      <StatusChip
                        label={t(`status.${diStatusToI18nKey(s.status)}` as const)}
                        tone={toneOf(DI_STATUS_TONE, s.status)}
                        dot
                      />
                      <span className="text-text-muted">{shortDate(s.submitted_at)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {/* Full description (the header only shows two lines) */}
          {di.description?.trim() ? (
            <section aria-labelledby="di-detail-description" className="space-y-1">
              <h3 id="di-detail-description" className="text-xs font-semibold text-text-muted">
                {t("detail.fields.description")}
              </h3>
              <p className="whitespace-pre-wrap break-words text-sm text-text-primary">
                {di.description}
              </p>
            </section>
          ) : null}

          {/* Info grid */}
          <Card>
            <CardContent className="p-3 grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-2 text-xs">
              <InfoRow
                icon={<Calendar className="h-3 w-3" aria-hidden />}
                label={t("detail.fields.reportedAt")}
                value={shortDate(di.submitted_at)}
              />
              <InfoRow
                icon={<User className="h-3 w-3" aria-hidden />}
                label={t("detail.fields.reportedBy")}
                value={formatPersonLabel(di.submitter_display_name)}
              />
              <InfoRow
                label={t("detail.fields.priority")}
                value={<UrgencyBadge code={di.reported_urgency} />}
              />
              {di.source_inspection_round_id != null && (
                <InfoRow
                  icon={<ClipboardCheck className="h-3 w-3" aria-hidden />}
                  label={t("detail.fields.sourceInspectionRound")}
                  value={
                    <LinkedEntityBadge
                      entity="inspection_round"
                      code={di.source_inspection_round_code}
                      entityId={di.source_inspection_round_id}
                      title={roundTitle(
                        di.source_inspection_round_title,
                        di.source_inspection_round_scheduled_at,
                        i18n.language,
                      )}
                    />
                  }
                />
              )}
              {di.validated_urgency != null && (
                <InfoRow
                  label={t("screening.validatedUrgency")}
                  value={<UrgencyBadge code={di.validated_urgency} />}
                />
              )}
              {di.status === "deferred" && di.deferred_until && (
                <InfoRow label={t("deferral.deferredUntil")} value={shortDate(di.deferred_until)} />
              )}
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
              {di.reviewer_display_name ? (
                <InfoRow
                  label={t("detail.fields.reviewer")}
                  value={formatPersonLabel(di.reviewer_display_name)}
                />
              ) : null}
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
              {slaStatus && slaStatus.paused_hours > 0 ? (
                <InfoRow
                  label={t("detail.fields.slaPaused")}
                  value={t("detail.fields.hoursValue", {
                    hours: Math.round(slaStatus.paused_hours * 10) / 10,
                  })}
                />
              ) : null}
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
            </CardContent>
          </Card>

          {di.reviewer_note?.trim() && di.status !== "returned_for_clarification" ? (
            <section aria-labelledby="di-detail-reviewer-note" className="space-y-1">
              <h3 id="di-detail-reviewer-note" className="text-xs font-semibold text-text-muted">
                {t("review.reviewerNote")}
              </h3>
              <p className="whitespace-pre-wrap break-words text-sm text-text-primary">
                {di.reviewer_note}
              </p>
            </section>
          ) : null}

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
        {actionError && (
          <div className="px-6 pt-3">
            <p role="alert" className="text-xs text-status-danger">
              {actionError}
            </p>
          </div>
        )}
        <div className="flex flex-wrap items-center justify-between gap-2 px-6 py-3">
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={handlePrint} className="gap-1.5">
              <Printer className="h-3.5 w-3.5" aria-hidden />
              {t("action.print")}
            </Button>
            {actions.edit && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => handOff((d) => openCreateForm(d))}
              >
                <Pencil className="h-3.5 w-3.5" aria-hidden />
                {t("contextMenu.edit")}
              </Button>
            )}
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            {actions.resubmit && (
              <Button size="sm" className="gap-1.5" onClick={() => setResubmitOpen(true)}>
                <MessageSquareReply className="h-3.5 w-3.5" aria-hidden />
                {t("action.resubmit")}
              </Button>
            )}
            {actions.triage && (
              <Button
                size="sm"
                className="gap-1.5"
                disabled={triageSaving}
                onClick={() => void handleTriageAccept()}
              >
                <ClipboardCheck className="h-3.5 w-3.5" aria-hidden />
                {t("triage.acceptForReview")}
              </Button>
            )}
            {actions.screen && (
              <Button size="sm" className="gap-1.5" onClick={() => setScreenOpen(true)}>
                <ClipboardCheck className="h-3.5 w-3.5" aria-hidden />
                {t("review.screenAction")}
              </Button>
            )}
            {actions.approve && (
              <Button size="sm" className="gap-1.5" onClick={() => handOff(openApproval)}>
                <Check className="h-3.5 w-3.5" aria-hidden />
                {t("action.approve")}
              </Button>
            )}
            {actions.convert && (
              <Button size="sm" className="gap-1.5" onClick={() => handOff(openApproval)}>
                <Check className="h-3.5 w-3.5" aria-hidden />
                {t("action.convertToWO")}
              </Button>
            )}
            {actions.returnForInfo && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => handOff(openReturn)}
              >
                <RotateCcw className="h-3.5 w-3.5" aria-hidden />
                {t("review.returnAction")}
              </Button>
            )}
            {actions.defer && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => setDeferOpen(true)}
              >
                <Calendar className="h-3.5 w-3.5" aria-hidden />
                {t("action.defer")}
              </Button>
            )}
            {actions.reactivate && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => setConfirmAction("reactivate")}
              >
                <CalendarClock className="h-3.5 w-3.5" aria-hidden />
                {t("action.reactivate")}
              </Button>
            )}
            {actions.close && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => handOff(openRejection)}
              >
                <X className="h-3.5 w-3.5" aria-hidden />
                {t("action.closeRequest")}
              </Button>
            )}
            {actions.archive && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => setConfirmAction("archive")}
              >
                <Archive className="h-3.5 w-3.5" aria-hidden />
                {t("action.archive")}
              </Button>
            )}
            {actions.cancelOwn && (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => setConfirmAction("cancelOwn")}
              >
                <X className="h-3.5 w-3.5" aria-hidden />
                {t("action.cancelOwn")}
              </Button>
            )}
            <Button variant="ghost" size="sm" onClick={onClose}>
              {t("detail.close")}
            </Button>
          </div>
        </div>
      </DialogContent>

      <DiScreenDialog
        di={di}
        open={screenOpen}
        onOpenChange={setScreenOpen}
        onScreened={handleScreened}
      />
      <DiDeferDialog
        di={di}
        open={deferOpen}
        onOpenChange={setDeferOpen}
        onDeferred={() => {
          refreshQueues();
          void openDi(di.id);
        }}
      />
      {actions.resubmit ? (
        <DiResubmitDialog
          di={di}
          question={question}
          open={resubmitOpen}
          onOpenChange={setResubmitOpen}
        />
      ) : null}
      <ConfirmDialog
        open={confirmAction != null}
        onOpenChange={(next) => !next && setConfirmAction(null)}
        title={
          confirmAction === "archive"
            ? t("confirm.archiveTitle", { code: di.code })
            : confirmAction === "reactivate"
              ? t("confirm.reactivateTitle", { code: di.code })
              : t("confirm.cancelOwnTitle", { code: di.code })
        }
        description={
          confirmAction === "archive"
            ? t("confirm.archiveDescription")
            : confirmAction === "reactivate"
              ? t("confirm.reactivateDescription")
              : t("confirm.cancelOwnDescription")
        }
        confirmLabel={
          confirmAction === "archive"
            ? t("action.archive")
            : confirmAction === "reactivate"
              ? t("action.reactivate")
              : t("action.cancelOwn")
        }
        destructive={confirmAction === "cancelOwn"}
        onConfirm={
          confirmAction === "archive"
            ? handleArchive
            : confirmAction === "reactivate"
              ? handleReactivate
              : handleCancelOwn
        }
      />
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

/**
 * Study step 6 — decision log: each decision with its evidence, status (proposed → approved →
 * applied, or rejected), what it created in maintenance and its measured effect.
 */
import { Check, ClipboardCheck, Play, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_DECISION_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import {
  type ReliabilityStudy,
  type StudyDecision,
  applyReliabilityStudyDecision,
  approveReliabilityStudyDecision,
  listReliabilityStudyDecisions,
  rejectReliabilityStudyDecision,
} from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

import { RAMS_TABLE_FRAME, RamsCard } from "./rams-ui";

export function StudyDecisionsStep({
  study,
  version,
  onRecord,
}: {
  study: ReliabilityStudy;
  version: number;
  onRecord: () => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatNumber, formatDecimal, formatDate } = useFormatters();
  const { can } = usePermissions();
  const { info } = useSession();
  const me = info?.user_id ?? null;
  const [rows, setRows] = useState<StudyDecision[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [applyTarget, setApplyTarget] = useState<StudyDecision | null>(null);
  const [rejectTarget, setRejectTarget] = useState<StudyDecision | null>(null);
  const [approveTarget, setApproveTarget] = useState<StudyDecision | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await listReliabilityStudyDecisions(study.id));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [study.id]);

  useEffect(() => {
    void load();
  }, [load, version]);

  // An archived study is a record: its decisions are no longer approved, rejected or applied.
  const canManage = can(P.RAM_MANAGE) && study.status === "active";
  /**
   * Applying also writes in another module: a new preventive interval needs pm.edit, any other
   * decision creates a work order (ot.create). Without that right the decision says so.
   */
  const canApply = (d: StudyDecision) =>
    d.action_kind === "pm_interval" ? can(P.PM_EDIT) : can(P.OT_CREATE);
  const hours = (v: number | null | undefined) => (v == null ? "—" : formatNumber(Math.round(v)));
  /** Another person was designated: only they approve or reject. */
  const designatedElsewhere = (d: StudyDecision) => d.approver_id != null && d.approver_id !== me;
  /** Four-eyes: the proposer does not approve their own decision, unless its site opted out. */
  const ownProposal = (d: StudyDecision) =>
    !d.site_allows_self_approval && d.proposed_by_id != null && d.proposed_by_id === me;
  /**
   * The proposer may withdraw their own proposal until it is decided, whoever was designated to
   * approve it (owner decision D10): with the right they proposed with, or the approve right.
   */
  const canWithdraw = (d: StudyDecision) =>
    d.status === "proposed" &&
    d.proposed_by_id != null &&
    d.proposed_by_id === me &&
    (can(P.RAM_ANALYZE) || can(P.RAM_MANAGE)) &&
    study.status === "active";
  /** Reject (as the approver) or withdraw (as the proposer). */
  const canReject = (d: StudyDecision) =>
    d.status === "proposed" && ((canManage && !designatedElsewhere(d)) || canWithdraw(d));
  /** The proposer's own rejection is a withdrawal: "Retirer", shown "Retirée" once done. */
  const isMine = (d: StudyDecision | null) =>
    d != null && d.proposed_by_id != null && d.proposed_by_id === me;
  const statusKey = (d: StudyDecision) =>
    d.status === "rejected" && d.proposed_by_id != null && d.decided_by_id === d.proposed_by_id
      ? "withdrawn"
      : d.status;

  /** Runs inside the confirmation: a refusal (not the designated approver…) keeps it open. */
  const approve = async (d: StudyDecision) => {
    await approveReliabilityStudyDecision({
      decision_id: d.id,
      expected_row_version: d.row_version,
    });
    await load();
  };

  if (error != null && rows == null)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (rows == null) return <LoadingState />;

  const evidence = (d: StudyDecision) => {
    const e = d.evidence;
    const parts: string[] = [];
    if (e.beta != null) parts.push(t("rams.decisions.evBeta", { beta: formatDecimal(e.beta, 2) }));
    if (e.b10 != null) parts.push(t("rams.decisions.evB10", { b10: hours(e.b10) }));
    parts.push(t("rams.decisions.evFailures", { count: e.n_failures }));
    if (e.annual_saving_vs_current != null)
      parts.push(
        t("rams.decisions.evSaving", {
          saving: formatNumber(Math.round(e.annual_saving_vs_current)),
          currency: e.currency_code ?? "",
        }),
      );
    return parts.join(" · ");
  };

  const effect = (d: StudyDecision) => {
    const ef = d.effect;
    if (ef.state === "measured" && ef.change != null) {
      const pct = formatDecimal(ef.change * 100, 0);
      return (
        <StatusChip
          tone={ef.change >= 0 ? "success" : "danger"}
          label={t(
            ef.after_is_lower_bound
              ? "rams.decisions.effectAtLeast"
              : "rams.decisions.effectMeasured",
            {
              change: `${ef.change >= 0 ? "+" : ""}${pct}`,
            },
          )}
          title={t("rams.decisions.effectDetail", {
            before: hours(ef.mtbf_before),
            after: hours(ef.mtbf_after),
            failures: ef.failures_after,
          })}
        />
      );
    }
    return <span className="text-text-muted">{t(`rams.decisions.effect.${ef.state}`)}</span>;
  };

  return (
    <RamsCard
      title={t("rams.decisions.title")}
      subtitle={t("rams.decisions.hint")}
      headingId="rams-decisions"
      actions={
        can(P.RAM_ANALYZE) && study.status === "active" ? (
          <Button className={mfLayout.primaryAction} onClick={onRecord}>
            <ClipboardCheck className="h-4 w-4" aria-hidden />
            {t("rams.study.recordDecision")}
          </Button>
        ) : null
      }
    >
      {rows.length === 0 ? (
        <EmptyState
          title={t("rams.decisions.emptyTitle")}
          description={t("rams.decisions.emptyBody")}
        />
      ) : (
        <div className={RAMS_TABLE_FRAME}>
          <div className="overflow-x-auto">
            <table className="w-full text-sm" aria-labelledby="rams-decisions">
              <thead>
                <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                  <th className="px-3 py-2 text-start">{t("rams.decisions.colDecision")}</th>
                  <th className="px-3 py-2 text-start">{t("rams.decisions.colEvidence")}</th>
                  <th className="px-3 py-2 text-start">{t("rams.decisions.colStatus")}</th>
                  <th className="px-3 py-2 text-start">{t("rams.decisions.colCreated")}</th>
                  <th className="px-3 py-2 text-start">{t("rams.decisions.colEffect")}</th>
                  <th className="px-3 py-2 text-end">
                    <span className="sr-only">{t("rams.decisions.colActions")}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((d) => (
                  <tr key={d.id} className="border-t border-surface-border align-top">
                    <td className="px-3 py-2">
                      <p className="font-medium text-text-primary">{d.title}</p>
                      <p className="text-xs text-text-muted">
                        {t(`rams.decision.kinds.${d.action_kind}`)}
                        {d.action_kind === "pm_interval" &&
                        d.proposed_trigger_interval_value != null
                          ? ` · ${t("rams.decisions.newInterval", {
                              value: formatNumber(d.proposed_trigger_interval_value),
                              unit: t(
                                `rams.units.interval.${d.proposed_trigger_interval_unit ?? "operating_hours"}`,
                              ),
                            })}`
                          : ""}
                        {d.asset_code ? ` · ${d.asset_code}` : ""}
                      </p>
                      <p className="text-xs text-text-muted">
                        {t("rams.decisions.proposedBy", {
                          name: d.proposed_by_name ?? "—",
                          date: formatDate(d.proposed_at),
                        })}
                        {d.approver_name
                          ? ` · ${t("rams.decisions.approver", { name: d.approver_name })}`
                          : ""}
                      </p>
                    </td>
                    <td className="px-3 py-2 text-xs text-text-secondary">{evidence(d)}</td>
                    <td className="px-3 py-2">
                      <StatusChip
                        dot
                        tone={toneOf(RELIABILITY_DECISION_STATUS_TONE, d.status)}
                        label={t(`rams.decisions.status.${statusKey(d)}`)}
                      />
                      {d.sync_refused_at != null ? (
                        <StatusChip
                          className="ml-1"
                          tone="danger"
                          label={t("rams.decisions.syncRefused")}
                          title={t("rams.decisions.syncRefusedHint")}
                        />
                      ) : null}
                      {d.sync_refused_at != null && d.sync_refused_reason ? (
                        <p className="mt-1 text-xs text-text-muted">{d.sync_refused_reason}</p>
                      ) : null}
                      {d.self_approved ? (
                        <StatusChip
                          className="ml-1"
                          tone="warning"
                          label={t("rams.decisions.selfApproved")}
                          title={t("rams.decisions.selfApprovedHint")}
                        />
                      ) : null}
                      {d.decision_note ? (
                        <p className="mt-1 text-xs text-text-muted">{d.decision_note}</p>
                      ) : null}
                    </td>
                    <td className="px-3 py-2">
                      <div className="flex flex-wrap gap-1">
                        {d.linked_pm_plan_version_id != null && d.pm_plan_code ? (
                          <LinkedEntityBadge
                            entity="pm_plan"
                            code={`${d.pm_plan_code} v${d.linked_pm_version_no ?? ""}`}
                            entityId={d.linked_pm_plan_id}
                          />
                        ) : null}
                        {d.linked_work_order_code ? (
                          <LinkedEntityBadge
                            entity="work_order"
                            code={d.linked_work_order_code}
                            entityId={d.linked_work_order_id}
                          />
                        ) : null}
                        {d.linked_pm_plan_version_id == null && d.linked_work_order_code == null ? (
                          <span className="text-xs text-text-muted">
                            {t("rams.decisions.nothingYet")}
                          </span>
                        ) : null}
                      </div>
                    </td>
                    <td className="px-3 py-2">{effect(d)}</td>
                    <td className="px-3 py-2">
                      {canManage || canWithdraw(d) ? (
                        <div className="flex justify-end gap-1.5">
                          {canManage && d.status === "proposed" && designatedElsewhere(d) ? (
                            <span className="self-center text-xs text-text-muted">
                              {t("rams.decisions.awaitingApprover", {
                                name: d.approver_name ?? "—",
                              })}
                            </span>
                          ) : null}
                          {canManage &&
                          d.status === "proposed" &&
                          d.sync_refused_at == null &&
                          !designatedElsewhere(d) ? (
                            ownProposal(d) ? (
                              <span className="self-center text-xs text-text-muted">
                                {t("rams.decisions.ownProposal")}
                              </span>
                            ) : (
                              <Button
                                variant="outline"
                                className="h-10"
                                disabled={busy}
                                onClick={() => setApproveTarget(d)}
                              >
                                <Check className="h-4 w-4" aria-hidden />
                                {t("rams.decisions.approve")}
                              </Button>
                            )
                          ) : null}
                          {canReject(d) ? (
                            <Button
                              variant="ghost"
                              className="h-10"
                              disabled={busy}
                              onClick={() => {
                                setNote("");
                                setRejectTarget(d);
                              }}
                            >
                              <X className="h-4 w-4" aria-hidden />
                              {t(isMine(d) ? "rams.decisions.withdraw" : "rams.decisions.reject")}
                            </Button>
                          ) : null}
                          {canManage &&
                          d.status === "approved" &&
                          d.sync_refused_at == null &&
                          canApply(d) ? (
                            <Button
                              className="h-10"
                              disabled={busy}
                              onClick={() => setApplyTarget(d)}
                            >
                              <Play className="h-4 w-4" aria-hidden />
                              {t("rams.decisions.apply")}
                            </Button>
                          ) : null}
                          {canManage &&
                          d.status === "approved" &&
                          d.sync_refused_at == null &&
                          !canApply(d) ? (
                            <span className="text-xs text-text-muted" role="note">
                              {t(
                                d.action_kind === "pm_interval"
                                  ? "rams.decisions.applyNeedsPmEdit"
                                  : "rams.decisions.applyNeedsWoCreate",
                              )}
                            </span>
                          ) : null}
                        </div>
                      ) : null}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
      <ConfirmDialog
        open={approveTarget != null}
        onOpenChange={(o) => !o && setApproveTarget(null)}
        title={t("rams.decisions.approveTitle")}
        description={t("rams.decisions.approveBody", { title: approveTarget?.title ?? "" })}
        confirmLabel={t("rams.decisions.approve")}
        onConfirm={async () => {
          if (approveTarget) await approve(approveTarget);
        }}
      />
      <ConfirmDialog
        open={applyTarget != null}
        onOpenChange={(o) => !o && setApplyTarget(null)}
        title={t("rams.decisions.applyTitle")}
        description={
          applyTarget?.action_kind === "pm_interval"
            ? t("rams.decisions.applyPm", { plan: applyTarget.pm_plan_code ?? "" })
            : t("rams.decisions.applyWo", { asset: applyTarget?.asset_code ?? "" })
        }
        confirmLabel={
          applyTarget?.action_kind === "pm_interval"
            ? t("rams.decisions.applyPmAction")
            : t("rams.decisions.applyWoAction")
        }
        onConfirm={async () => {
          if (!applyTarget) return;
          await applyReliabilityStudyDecision({
            decision_id: applyTarget.id,
            expected_row_version: applyTarget.row_version,
          });
          await load();
        }}
      />
      <Dialog open={rejectTarget != null} onOpenChange={(o) => !o && setRejectTarget(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {t(
                isMine(rejectTarget)
                  ? "rams.decisions.withdrawTitle"
                  : "rams.decisions.rejectTitle",
              )}
            </DialogTitle>
            <DialogDescription>{rejectTarget?.title}</DialogDescription>
          </DialogHeader>
          <label className="grid gap-1 text-sm font-medium text-text-secondary">
            {t(
              isMine(rejectTarget)
                ? "rams.decisions.withdrawReason"
                : "rams.decisions.rejectReason",
            )}
            <Textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
          </label>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setRejectTarget(null)}>
              {t("rams.common.cancel")}
            </Button>
            <Button
              variant="destructive"
              className="h-11"
              disabled={note.trim().length === 0 || busy}
              onClick={async () => {
                if (!rejectTarget) return;
                setBusy(true);
                try {
                  await rejectReliabilityStudyDecision({
                    decision_id: rejectTarget.id,
                    expected_row_version: rejectTarget.row_version,
                    note: note.trim(),
                  });
                  setRejectTarget(null);
                  await load();
                } catch {
                  // global IPC toast
                } finally {
                  setBusy(false);
                }
              }}
            >
              {t(
                isMine(rejectTarget)
                  ? "rams.decisions.withdrawAction"
                  : "rams.decisions.rejectAction",
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </RamsCard>
  );
}

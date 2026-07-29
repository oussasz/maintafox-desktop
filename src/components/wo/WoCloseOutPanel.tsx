/**
 * WoCloseOutPanel.tsx
 *
 * Close-out panel for a Work Order (Option B):
 *   S1 — Symptom & Narrative
 *   S2 — Failure Analysis (mode / cause / effect)
 *   S3 — Action Performed (corrective action, root cause, repair type, service cost)
 *   S4 — Supervisor verification → closure (and reopen from completed)
 */

import {
  AlertCircle,
  CheckCircle2,
  Loader2,
  Save,
  WrenchIcon,
  ShieldCheck,
  Lock,
  RotateCcw,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  WoCompletionGatesChecklist,
  completionGatesProgress,
} from "@/components/wo/WoCompletionGatesChecklist";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { useStepUp } from "@/hooks/use-step-up";
import { formatPersonLabel } from "@/lib/display";
import {
  saveFailureDetail,
  saveVerification,
  closeWo,
  reopenWo,
  updateWoRca,
  updateServiceCost,
  getWoAnalyticsSnapshot,
  CloseoutBlockingError,
} from "@/services/wo-closeout-service";
import { evaluateWoCompletionGates } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { toErrorMessage } from "@/utils/errors";
import type { WoCompletionGate, WorkOrder } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoCloseOutPanelProps {
  wo: WorkOrder;
  canEdit: boolean;
  onClosed: () => void;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

type RepairType = "temporary" | "permanent" | "na";

/** Option B: verification and closure available from completed */
const VERIFIABLE_STATUSES = new Set(["completed"]);
const CLOSEABLE_STATUSES = new Set(["completed"]);
const REOPENABLE_STATUSES = new Set(["completed"]);

function SectionHeader({ icon, title }: { icon: React.ReactNode; title: string }) {
  return (
    <div className="flex items-center gap-2 border-b border-surface-border pb-2">
      <span className="text-primary">{icon}</span>
      <h3 className="text-sm font-semibold text-text-primary">{title}</h3>
    </div>
  );
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}

function SuccessBanner({ message }: { message: string }) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-green-500/50 bg-green-50 p-3 text-sm text-green-700">
      <CheckCircle2 className="h-4 w-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCloseOutPanel({ wo, canEdit, onClosed }: WoCloseOutPanelProps) {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const woItems = useWoStore((s) => s.items);
  const canReopen = can("ot.admin");

  // Track current WO state (gets updated after transitions)
  const [currentWo, setCurrentWo] = useState<WorkOrder>(wo);
  const statusCode = currentWo.status_code ?? "draft";

  // ── S1 + S2 + S3 form state ────────────────────────────────────────────
  const [symptomId, setSymptomId] = useState<number | null>(null);
  const [narrative, setNarrative] = useState("");
  const [causeNotDetermined, setCauseNotDetermined] = useState(false);
  const [failureModeId, setFailureModeId] = useState<number | null>(null);
  const [failureCauseId, setFailureCauseId] = useState<number | null>(null);
  const [failureEffectId, setFailureEffectId] = useState<number | null>(null);
  const [repairType, setRepairType] = useState<RepairType>("na");
  const [correctiveAction, setCorrectiveAction] = useState("");
  const [rootCauseSummary, setRootCauseSummary] = useState("");
  const [serviceCost, setServiceCost] = useState("");

  const [savingDetails, setSavingDetails] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [detailSuccess, setDetailSuccess] = useState(false);

  // ── S4 — Verification form ────────────────────────────────────────────
  const [verificationResult, setVerificationResult] = useState<"pass" | "fail" | "monitor" | "">(
    "",
  );
  const [returnToServiceConfirmed, setReturnToServiceConfirmed] = useState(false);
  const [verificationNotes, setVerificationNotes] = useState("");
  const [savingVerification, setSavingVerification] = useState(false);
  const [verificationError, setVerificationError] = useState<string | null>(null);

  // ── S4 — Close ────────────────────────────────────────────────────────
  const [closing, setClosing] = useState(false);
  const [blockingErrors, setBlockingErrors] = useState<string[]>([]);
  const [closeError, setCloseError] = useState<string | null>(null);
  const [noDowntimeAttestation, setNoDowntimeAttestation] = useState(false);
  const [noDowntimeReason, setNoDowntimeReason] = useState("");
  const [fmecaOverrideReason, setFmecaOverrideReason] = useState("");
  const [fmecaOverrideSignerId, setFmecaOverrideSignerId] = useState("");
  const [fmecaOverrideSignerPassword, setFmecaOverrideSignerPassword] = useState("");

  // ── Reopen (completed only) ───────────────────────────────────────────
  const [reopening, setReopening] = useState(false);
  const [reopenReason, setReopenReason] = useState("");
  const [reopenTarget, setReopenTarget] = useState<"in_progress" | "planning">("in_progress");
  const [reopenError, setReopenError] = useState<string | null>(null);
  const [completionGates, setCompletionGates] = useState<WoCompletionGate[]>([]);

  const signerOptions = useMemo(() => {
    const dedup = new Map<string, string>();
    for (const item of woItems) {
      if (item.primary_responsible_id != null) {
        dedup.set(
          String(item.primary_responsible_id),
          item.responsible_display_name?.trim() ||
            item.responsible_username?.trim() ||
            String(item.primary_responsible_id),
        );
      }
      if (item.planner_id != null) {
        dedup.set(
          String(item.planner_id),
          item.planner_display_name?.trim() ||
            item.planner_username?.trim() ||
            String(item.planner_id),
        );
      }
    }
    if (info?.user_id != null) {
      dedup.set(String(info.user_id), formatPersonLabel(info.display_name, info.username));
    }
    return Array.from(dedup.entries())
      .map(([value, label]) => ({ value, label }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [woItems, info?.user_id, info?.display_name, info?.username]);

  // ── Pre-populate from existing failure data (GA-049) ────────────────
  useEffect(() => {
    let cancelled = false;
    void getWoAnalyticsSnapshot(wo.id)
      .then((snap) => {
        if (cancelled) return;
        const fd = snap.failure_details[0];
        if (fd) {
          setSymptomId(fd.symptom_id);
          setFailureModeId(fd.failure_mode_id);
          setFailureCauseId(fd.failure_cause_id);
          setFailureEffectId(fd.failure_effect_id);
          setCauseNotDetermined(fd.cause_not_determined);
          if (fd.is_temporary_repair) setRepairType("temporary");
          else if (fd.is_permanent_repair) setRepairType("permanent");
          if (fd.notes) setNarrative(fd.notes);
        }
        if (snap.root_cause_summary) setRootCauseSummary(snap.root_cause_summary);
        if (snap.corrective_action_summary) setCorrectiveAction(snap.corrective_action_summary);
        if (snap.service_cost > 0) setServiceCost(String(snap.service_cost));
      })
      .catch(() => {
        // Analytics not available yet — continue with empty form
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id]);

  useEffect(() => {
    let cancelled = false;
    void evaluateWoCompletionGates(wo.id)
      .then((rows) => {
        if (!cancelled) setCompletionGates(rows);
      })
      .catch(() => {
        if (!cancelled) setCompletionGates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id, currentWo.row_version, rootCauseSummary, failureModeId, symptomId, causeNotDetermined]);

  // Keep currentWo in sync with prop changes (e.g., parent refreshes)
  useEffect(() => {
    setCurrentWo(wo);
  }, [wo]);

  // ── Save S1-S3 details ─────────────────────────────────────────────────

  const handleSaveDetails = useCallback(async () => {
    if (!canEdit) return;
    setSavingDetails(true);
    setDetailError(null);
    setDetailSuccess(false);
    try {
      await saveFailureDetail({
        wo_id: currentWo.id,
        symptom_id: symptomId,
        failure_mode_id: causeNotDetermined ? null : failureModeId,
        failure_cause_id: causeNotDetermined ? null : failureCauseId,
        failure_effect_id: causeNotDetermined ? null : failureEffectId,
        is_temporary_repair: repairType === "temporary",
        is_permanent_repair: repairType === "permanent",
        cause_not_determined: causeNotDetermined,
        notes: narrative.trim() || null,
      });
      await updateWoRca({
        wo_id: currentWo.id,
        root_cause_summary: rootCauseSummary.trim() || null,
        corrective_action_summary: correctiveAction.trim() || null,
      });
      const costNum = parseFloat(serviceCost);
      if (!isNaN(costNum) && costNum >= 0) {
        await updateServiceCost(currentWo.id, costNum);
      }
      setDetailSuccess(true);
      await refreshActiveWo();
      try {
        setCompletionGates(await evaluateWoCompletionGates(currentWo.id));
      } catch {
        /* keep previous gates */
      }
    } catch (err) {
      setDetailError(toErrorMessage(err));
    } finally {
      setSavingDetails(false);
    }
  }, [
    canEdit,
    currentWo,
    symptomId,
    causeNotDetermined,
    failureModeId,
    failureCauseId,
    failureEffectId,
    repairType,
    narrative,
    rootCauseSummary,
    correctiveAction,
    serviceCost,
    refreshActiveWo,
  ]);

  // ── Submit verification ────────────────────────────────────────────────

  const handleSaveVerification = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !actorId || !verificationResult) return;
    setSavingVerification(true);
    setVerificationError(null);
    withStepUp(() =>
      saveVerification({
        wo_id: currentWo.id,
        verified_by_id: actorId,
        result: verificationResult,
        return_to_service_confirmed: returnToServiceConfirmed,
        recurrence_risk_level: null,
        notes: verificationNotes.trim() || null,
        expected_row_version: currentWo.row_version,
      }),
    )
      .then(async ([, updatedWo]) => {
        setCurrentWo(updatedWo);
        setVerificationResult("");
        setReturnToServiceConfirmed(false);
        setVerificationNotes("");
        await refreshActiveWo();
      })
      .catch((err: unknown) => setVerificationError(toErrorMessage(err)))
      .finally(() => setSavingVerification(false));
  }, [
    canEdit,
    currentWo,
    info,
    withStepUp,
    verificationResult,
    returnToServiceConfirmed,
    verificationNotes,
    refreshActiveWo,
  ]);

  // ── Close WO ───────────────────────────────────────────────────────────

  const handleClose = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !actorId) return;
    setClosing(true);
    setBlockingErrors([]);
    setCloseError(null);
    withStepUp(() =>
      closeWo({
        wo_id: currentWo.id,
        actor_id: actorId,
        expected_row_version: currentWo.row_version,
        ...(currentWo.production_impact_id != null
          ? {
              no_downtime_attestation: noDowntimeAttestation,
              no_downtime_attestation_reason: noDowntimeReason.trim() || null,
            }
          : {}),
        fmeca_parts_override_reason: fmecaOverrideReason.trim() || null,
        fmeca_parts_override_signed_by_id:
          fmecaOverrideSignerId.trim() !== "" ? Number(fmecaOverrideSignerId) : null,
        fmeca_parts_override_signer_password: fmecaOverrideSignerPassword || null,
      }),
    )
      .then(() => onClosed())
      .catch((err: unknown) => {
        if (err instanceof CloseoutBlockingError) {
          setBlockingErrors(err.blockingErrors);
        } else {
          setCloseError(toErrorMessage(err));
        }
      })
      .finally(() => setClosing(false));
  }, [
    canEdit,
    currentWo,
    info,
    withStepUp,
    onClosed,
    noDowntimeAttestation,
    noDowntimeReason,
    fmecaOverrideReason,
    fmecaOverrideSignerId,
    fmecaOverrideSignerPassword,
  ]);

  const handleReopen = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !canReopen || !actorId) return;
    const reason = reopenReason.trim();
    if (!reason) {
      setReopenError(t("closeout.reopenReasonRequired"));
      return;
    }
    setReopening(true);
    setReopenError(null);
    withStepUp(() =>
      reopenWo({
        wo_id: currentWo.id,
        actor_id: actorId,
        expected_row_version: currentWo.row_version,
        reason,
        target_status: reopenTarget,
      }),
    )
      .then(async (updated) => {
        setCurrentWo(updated);
        setReopenReason("");
        await refreshActiveWo();
      })
      .catch((err: unknown) => setReopenError(toErrorMessage(err)))
      .finally(() => setReopening(false));
  }, [
    canEdit,
    canReopen,
    info?.user_id,
    reopenReason,
    reopenTarget,
    currentWo.id,
    currentWo.row_version,
    refreshActiveWo,
    withStepUp,
    t,
  ]);

  // ── Render ──────────────────────────────────────────────────────────────

  const isDetailEditable = canEdit && !["closed", "cancelled"].includes(statusCode);
  const closeoutProgress = completionGatesProgress(completionGates);

  return (
    <div className="space-y-6">
      {StepUpDialogElement}

      {completionGates.length > 0 && (
        <section className="space-y-2">
          <div className="flex items-center justify-between gap-2">
            <h3 className="text-sm font-semibold">{t("closeout.progressTitle")}</h3>
            <span className="text-xs text-muted-foreground">
              {t("closeout.progressBadge", {
                done: closeoutProgress.done,
                total: closeoutProgress.total,
              })}
            </span>
          </div>
          <WoCompletionGatesChecklist
            gates={completionGates}
            showBlockingCount={false}
          />
        </section>
      )}

      {/* ══ Section 1 — Symptom & Narrative ══════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader
          icon={<AlertCircle className="h-4 w-4" />}
          title={t("closeout.sectionSymptom")}
        />

        <div className="space-y-2">
          <Label htmlFor="wo-symptom">{t("closeout.observedSymptom")}</Label>
          <ReferenceCombobox
            id="wo-symptom"
            referenceType="work.symptom"
            valueMode="id"
            value={symptomId != null ? String(symptomId) : null}
            onChange={(idStr) => setSymptomId(idStr ? Number(idStr) : null)}
            allowClear
            allowCreate={false}
            placeholder={t("closeout.selectSymptom")}
            disabled={!isDetailEditable}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-narrative">{t("closeout.narrative")}</Label>
          <Textarea
            id="wo-narrative"
            placeholder={t("closeout.narrativePlaceholder")}
            value={narrative}
            onChange={(e) => setNarrative(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>
      </section>

      {/* ══ Section 2 — Failure Analysis ═════════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader
          icon={<WrenchIcon className="h-4 w-4" />}
          title={t("closeout.sectionFailure")}
        />
        <p className="text-xs text-muted-foreground">{t("closeout.isoFailureTaxonomyHint")}</p>

        <div className="flex items-center gap-2">
          <Checkbox
            id="wo-cause-nd"
            checked={causeNotDetermined}
            onCheckedChange={(v) => setCauseNotDetermined(Boolean(v))}
            disabled={!isDetailEditable}
          />
          <Label htmlFor="wo-cause-nd" className="cursor-pointer">
            {t("closeout.causeNotDetermined")}
          </Label>
        </div>

        <div className="grid gap-4 sm:grid-cols-3">
          <div className="space-y-2">
            <Label htmlFor="wo-failure-mode">{t("closeout.failureMode")}</Label>
            <ReferenceCombobox
              id="wo-failure-mode"
              referenceType="work.failure_mode"
              valueMode="id"
              value={failureModeId != null ? String(failureModeId) : null}
              onChange={(idStr) => setFailureModeId(idStr ? Number(idStr) : null)}
              allowClear
              allowCreate={false}
              placeholder={t("closeout.failureModePlaceholder")}
              disabled={!isDetailEditable || causeNotDetermined}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="wo-failure-cause">{t("closeout.failureCause")}</Label>
            <ReferenceCombobox
              id="wo-failure-cause"
              referenceType="work.failure_cause"
              valueMode="id"
              value={failureCauseId != null ? String(failureCauseId) : null}
              onChange={(idStr) => setFailureCauseId(idStr ? Number(idStr) : null)}
              allowClear
              allowCreate={false}
              placeholder={t("closeout.failureCausePlaceholder")}
              disabled={!isDetailEditable || causeNotDetermined}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="wo-failure-effect">{t("closeout.failureEffect")}</Label>
            <ReferenceCombobox
              id="wo-failure-effect"
              referenceType="work.failure_effect"
              valueMode="id"
              value={failureEffectId != null ? String(failureEffectId) : null}
              onChange={(idStr) => setFailureEffectId(idStr ? Number(idStr) : null)}
              allowClear
              allowCreate={false}
              placeholder={t("closeout.failureEffectPlaceholder")}
              disabled={!isDetailEditable || causeNotDetermined}
            />
          </div>
        </div>
      </section>

      {/* ══ Section 3 — Action Performed ═════════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader icon={<Save className="h-4 w-4" />} title={t("closeout.sectionAction")} />

        <div className="space-y-2">
          <Label htmlFor="wo-corrective-action">{t("closeout.correctiveAction")}</Label>
          <Textarea
            id="wo-corrective-action"
            placeholder={t("closeout.correctiveActionPlaceholder")}
            value={correctiveAction}
            onChange={(e) => setCorrectiveAction(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-root-cause">{t("closeout.rootCause")}</Label>
          <Textarea
            id="wo-root-cause"
            placeholder={t("closeout.rootCausePlaceholder")}
            value={rootCauseSummary}
            onChange={(e) => setRootCauseSummary(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>

        <div className="space-y-2">
          <Label>{t("closeout.repairType")}</Label>
          <div className="flex gap-4">
            {(["temporary", "permanent", "na"] as const).map((val) => (
              <label key={val} className="flex cursor-pointer items-center gap-1.5">
                <input
                  type="radio"
                  name="repair-type"
                  value={val}
                  checked={repairType === val}
                  onChange={() => setRepairType(val)}
                  disabled={!isDetailEditable}
                  className="accent-primary"
                />
                <span className="text-sm">
                  {val === "temporary"
                    ? t("closeout.temporary")
                    : val === "permanent"
                      ? t("closeout.permanent")
                      : t("closeout.notApplicable")}
                </span>
              </label>
            ))}
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-service-cost">{t("closeout.serviceCost")}</Label>
          <Input
            id="wo-service-cost"
            type="number"
            min={0}
            step={0.01}
            placeholder="0.00"
            value={serviceCost}
            onChange={(e) => setServiceCost(e.target.value)}
            disabled={!isDetailEditable}
            className="w-40"
          />
        </div>

        {/* Save S1-S3 */}
        {isDetailEditable && (
          <div className="space-y-2">
            {detailError && <ErrorBanner message={detailError} />}
            {detailSuccess && <SuccessBanner message={t("closeout.detailsSaved")} />}
            <Button onClick={() => void handleSaveDetails()} disabled={savingDetails}>
              {savingDetails ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Save className="mr-2 h-4 w-4" />
              )}
              {t("closeout.saveDetails")}
            </Button>
          </div>
        )}
      </section>
      <section className="space-y-4">
        <SectionHeader
          icon={<ShieldCheck className="h-4 w-4" />}
          title={t("closeout.sectionReturn")}
        />

        {/* 4b — Technical verification form */}
        {VERIFIABLE_STATUSES.has(statusCode) && (
          <div className="space-y-4 rounded-md border border-surface-border p-4">
            <h4 className="text-sm font-medium text-text-primary">
              {t("closeout.technicalVerification")}
            </h4>

            <div className="space-y-2">
              <Label>{t("closeout.verificationResult")}</Label>
              <div className="flex gap-4">
                {(
                  [
                    { value: "pass", label: t("closeout.resultPass"), cls: "text-green-700" },
                    { value: "fail", label: t("closeout.resultFail"), cls: "text-destructive" },
                    { value: "monitor", label: t("closeout.resultMonitor"), cls: "text-amber-600" },
                  ] as const
                ).map(({ value, label, cls }) => (
                  <label key={value} className="flex cursor-pointer items-center gap-1.5">
                    <input
                      type="radio"
                      name="verification-result"
                      value={value}
                      checked={verificationResult === value}
                      onChange={() => setVerificationResult(value)}
                      disabled={!canEdit || savingVerification}
                      className="accent-primary"
                    />
                    <span className={`text-sm ${cls}`}>{label}</span>
                  </label>
                ))}
              </div>
            </div>

            {verificationResult === "pass" && (
              <div className="flex items-center gap-2">
                <Checkbox
                  id="wo-rts-confirmed"
                  checked={returnToServiceConfirmed}
                  onCheckedChange={(v) => setReturnToServiceConfirmed(Boolean(v))}
                  disabled={!canEdit || savingVerification}
                />
                <Label htmlFor="wo-rts-confirmed" className="cursor-pointer">
                  {t("closeout.returnToService")}
                </Label>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="wo-ver-notes">{t("closeout.verificationNotes")}</Label>
              <Textarea
                id="wo-ver-notes"
                placeholder={t("closeout.verificationNotesPlaceholder")}
                value={verificationNotes}
                onChange={(e) => setVerificationNotes(e.target.value)}
                disabled={!canEdit || savingVerification}
                rows={2}
              />
            </div>

            {verificationError && <ErrorBanner message={verificationError} />}

            <Button
              onClick={handleSaveVerification}
              disabled={
                !canEdit ||
                savingVerification ||
                !verificationResult ||
                (verificationResult === "pass" && !returnToServiceConfirmed)
              }
            >
              {savingVerification ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <ShieldCheck className="mr-2 h-4 w-4" />
              )}
              {t("closeout.submitVerification")}
            </Button>
          </div>
        )}

        {/* 4c — Closure */}
        {CLOSEABLE_STATUSES.has(statusCode) && (
          <div className="space-y-4 rounded-md border border-surface-border p-4">
            <h4 className="text-sm font-medium text-text-primary">{t("closeout.closureTitle")}</h4>
            <p className="text-sm text-muted-foreground">{t("closeout.closureWarning")}</p>

            {currentWo.production_impact_id != null && (
              <div className="space-y-3 rounded-md border border-surface-border bg-muted/30 p-3">
                <p className="text-sm text-muted-foreground">
                  {t("closeout.noDowntimeProductionHint")}
                </p>
                <div className="flex items-center gap-2">
                  <Checkbox
                    id="wo-no-dt-attest"
                    checked={noDowntimeAttestation}
                    onCheckedChange={(v) => setNoDowntimeAttestation(Boolean(v))}
                    disabled={!canEdit || closing}
                  />
                  <Label htmlFor="wo-no-dt-attest" className="cursor-pointer text-sm">
                    {t("closeout.noDowntimeAttestation")}
                  </Label>
                </div>
                {noDowntimeAttestation && (
                  <div className="space-y-1">
                    <Label htmlFor="wo-no-dt-reason">{t("closeout.noDowntimeReason")}</Label>
                    <Textarea
                      id="wo-no-dt-reason"
                      value={noDowntimeReason}
                      onChange={(e) => setNoDowntimeReason(e.target.value)}
                      disabled={!canEdit || closing}
                      rows={2}
                    />
                  </div>
                )}
              </div>
            )}
            <div className="space-y-1 rounded-md border border-surface-border bg-muted/20 p-3">
              <Label htmlFor="wo-fmeca-override-reason">{t("closeout.fmecaOverrideReason")}</Label>
              <Textarea
                id="wo-fmeca-override-reason"
                value={fmecaOverrideReason}
                onChange={(e) => setFmecaOverrideReason(e.target.value)}
                disabled={!canEdit || closing}
                rows={2}
                placeholder={t("closeout.fmecaOverrideReasonPlaceholder")}
              />
              <Label htmlFor="wo-fmeca-override-signer-id">{t("closeout.fmecaOverrideSignerId")}</Label>
              <Select
                value={fmecaOverrideSignerId || "__none"}
                onValueChange={(v) => setFmecaOverrideSignerId(v === "__none" ? "" : v)}
                disabled={!canEdit || closing}
              >
                <SelectTrigger id="wo-fmeca-override-signer-id">
                  <SelectValue placeholder={t("closeout.fmecaOverrideSignerIdPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none">—</SelectItem>
                  {signerOptions.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Label htmlFor="wo-fmeca-override-signer-password">
                {t("closeout.fmecaOverrideSignerPassword")}
              </Label>
              <Input
                id="wo-fmeca-override-signer-password"
                type="password"
                autoComplete="off"
                value={fmecaOverrideSignerPassword}
                onChange={(e) => setFmecaOverrideSignerPassword(e.target.value)}
                disabled={!canEdit || closing}
                placeholder={t("closeout.fmecaOverrideSignerPasswordPlaceholder")}
              />
              <p className="text-xs text-muted-foreground">{t("closeout.fmecaOverrideHint")}</p>
            </div>

            {/* Step-up authentication handled by withStepUp hook */}

            {/* Blocking errors from preflight */}
            {blockingErrors.length > 0 && (
              <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3">
                <p className="mb-2 text-sm font-medium text-destructive">
                  {t("closeout.blockingConditions")}
                </p>
                <ul className="space-y-1">
                  {blockingErrors.map((err, i) => (
                    <li key={i} className="flex items-start gap-2 text-sm text-destructive">
                      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                      <span>{err}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {closeError && <ErrorBanner message={closeError} />}

            <Button variant="destructive" onClick={handleClose} disabled={!canEdit || closing}>
              {closing ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Lock className="mr-2 h-4 w-4" />
              )}
              {t("closeout.closeWo")}
            </Button>
          </div>
        )}

        {REOPENABLE_STATUSES.has(statusCode) && canEdit && canReopen && (
          <div className="space-y-3 rounded-md border border-surface-border p-4">
            <h4 className="flex items-center gap-2 text-sm font-medium text-text-primary">
              <RotateCcw className="h-4 w-4" />
              {t("closeout.reopenTitle")}
            </h4>
            <p className="text-sm text-muted-foreground">{t("closeout.reopenHint")}</p>
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <Label>{t("closeout.reopenTarget")}</Label>
                <Select
                  value={reopenTarget}
                  onValueChange={(v) => setReopenTarget(v as "in_progress" | "planning")}
                  disabled={reopening}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="in_progress">{t("closeout.reopenTargetInProgress")}</SelectItem>
                    <SelectItem value="planning">{t("closeout.reopenTargetPlanning")}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label htmlFor="wo-reopen-reason">{t("closeout.reopenReason")}</Label>
                <Textarea
                  id="wo-reopen-reason"
                  value={reopenReason}
                  onChange={(e) => setReopenReason(e.target.value)}
                  disabled={reopening}
                  rows={2}
                  placeholder={t("closeout.reopenReasonPlaceholder")}
                />
              </div>
            </div>
            {reopenError && <ErrorBanner message={reopenError} />}
            <Button
              variant="outline"
              onClick={handleReopen}
              disabled={reopening || !reopenReason.trim()}
            >
              {reopening ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RotateCcw className="mr-2 h-4 w-4" />
              )}
              {t("closeout.reopenSubmit")}
            </Button>
          </div>
        )}
      </section>
    </div>
  );
}

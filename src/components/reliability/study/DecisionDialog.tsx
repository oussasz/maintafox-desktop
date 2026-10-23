/**
 * Record a decision of a reliability study: what is decided, how it is carried out (new PM plan
 * version with another interval, one-off or modification work order) and who approves it. The
 * evidence (fit, costs) is taken from the study by the backend when the decision is recorded.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { woTypeLabel } from "@/components/wo/use-wo-type-labels";
import { useFormatters } from "@/hooks/use-formatters";
import { useSession } from "@/hooks/use-session";
import {
  type DecisionKind,
  type PmPlanCandidate,
  type ReliabilityStudy,
  createReliabilityStudyDecision,
  getReliabilityStudyLifeAnalysis,
  listReliabilityDecisionApprovers,
} from "@/services/reliability-study-service";
import { listWorkOrderTypes } from "@/services/wo-service";
import type { WorkOrderTypeOption } from "@shared/ipc-types";

import { RAMS_SELECT_CLASS } from "./rams-ui";

export interface DecisionPrefill {
  kind: DecisionKind;
  pmPlanId: number | null;
  intervalHours: number | null;
  plannedCost: number | null;
  failureCost: number | null;
  title: string;
  /** FMECA row and RCM decision the decision comes from (steps 3 and 4). */
  fmecaItemId?: number | null;
  rcmDecisionId?: number | null;
}

const KINDS: DecisionKind[] = ["pm_interval", "one_off_wo", "modification_wo"];

export function DecisionDialog({
  open,
  onOpenChange,
  study,
  prefill,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  study: ReliabilityStudy;
  prefill: DecisionPrefill | null;
  onCreated: () => void;
}) {
  const { t, i18n } = useTranslation("reliability");
  const { formatNumber } = useFormatters();
  const { info } = useSession();
  const me = info?.user_id ?? null;
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState<DecisionKind>("pm_interval");
  const [plans, setPlans] = useState<PmPlanCandidate[]>([]);
  const [planId, setPlanId] = useState<number | null>(null);
  const [interval, setInterval] = useState<number | null>(null);
  const [equipmentId, setEquipmentId] = useState<number | null>(null);
  const [woTypes, setWoTypes] = useState<WorkOrderTypeOption[]>([]);
  const [woType, setWoType] = useState("");
  const [approvers, setApprovers] = useState<{ user_id: number; display_name: string }[]>([]);
  const [approver, setApprover] = useState<number | null>(null);
  const [justification, setJustification] = useState("");
  const [saving, setSaving] = useState(false);
  /** The plans, types or approvers could not be loaded: shown with a retry, never an empty form. */
  const [loadError, setLoadError] = useState<unknown>(null);
  const [loadRun, setLoadRun] = useState(0);

  useEffect(() => {
    if (!open) return;
    setLoadError(null);
    setTitle(prefill?.title ?? "");
    setKind(prefill?.kind ?? "pm_interval");
    setInterval(prefill?.intervalHours ?? null);
    setEquipmentId(study.assets.length === 1 ? (study.assets[0]?.equipment_id ?? null) : null);
    setJustification("");
    setApprover(null);
    let cancelled = false;
    void (async () => {
      try {
        const [a, types, people] = await Promise.all([
          getReliabilityStudyLifeAnalysis({
            study_id: study.id,
            planned_cost: prefill?.plannedCost ?? null,
            failure_cost: prefill?.failureCost ?? null,
            pm_plan_id: prefill?.pmPlanId ?? null,
          }),
          listWorkOrderTypes(),
          listReliabilityDecisionApprovers(),
        ]);
        if (cancelled) return;
        const usable = a.pm_plans.filter((p) => p.interval_hours != null);
        setPlans(usable);
        // The plan of the study's life analysis; otherwise the analyst chooses (no first-plan default).
        setPlanId(prefill?.pmPlanId ?? a.selected_pm_plan_id ?? null);
        const active = types.filter((x) => x.is_active);
        setWoTypes(active);
        // The work-order type is chosen by the analyst: none is preset.
        setWoType("");
        setApprovers(people);
        if (usable.length === 0 && (prefill?.kind ?? "pm_interval") === "pm_interval")
          setKind("one_off_wo");
      } catch (err) {
        if (!cancelled) setLoadError(err);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, prefill, study, loadRun]);

  const intervalN = interval ?? Number.NaN;
  const valid =
    loadError == null &&
    title.trim().length > 0 &&
    (kind === "pm_interval"
      ? planId != null && Number.isFinite(intervalN) && intervalN > 0
      : equipmentId != null && woType !== "");

  const submit = async () => {
    if (!valid) return;
    setSaving(true);
    try {
      await createReliabilityStudyDecision({
        study_id: study.id,
        title: title.trim(),
        action_kind: kind,
        pm_plan_id: kind === "pm_interval" ? planId : null,
        proposed_interval_hours: kind === "pm_interval" ? intervalN : null,
        equipment_id: kind === "pm_interval" ? null : equipmentId,
        wo_type_code: kind === "pm_interval" ? null : woType,
        approver_id: approver,
        justification: justification.trim() || null,
        fmeca_item_id: prefill?.fmecaItemId ?? null,
        rcm_decision_id: prefill?.rcmDecisionId ?? null,
        planned_cost: prefill?.plannedCost ?? null,
        failure_cost: prefill?.failureCost ?? null,
      });
      onOpenChange(false);
      onCreated();
    } catch {
      // global IPC toast; the dialog stays open
    } finally {
      setSaving(false);
    }
  };

  const field = "grid gap-1 text-xs font-medium text-text-secondary";
  return (
    <Dialog open={open} onOpenChange={(o) => !saving && onOpenChange(o)}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("rams.decision.dialogTitle")}</DialogTitle>
          <DialogDescription>{t("rams.decision.dialogHint")}</DialogDescription>
        </DialogHeader>
        {loadError != null ? (
          <ErrorState
            error={loadError}
            title={t("rams.decision.loadError")}
            onRetry={() => setLoadRun((n) => n + 1)}
            className="py-4"
          />
        ) : null}
        <div className="grid gap-3">
          <label className={field}>
            {t("rams.decision.title")}
            <Input className="h-10" value={title} onChange={(e) => setTitle(e.target.value)} />
          </label>
          <label className={field}>
            {t("rams.decision.kind")}
            <select
              className={RAMS_SELECT_CLASS}
              value={kind}
              onChange={(e) => setKind(e.target.value as DecisionKind)}
            >
              {KINDS.map((k) => (
                <option key={k} value={k} disabled={k === "pm_interval" && plans.length === 0}>
                  {t(`rams.decision.kinds.${k}`)}
                </option>
              ))}
            </select>
          </label>
          {kind === "pm_interval" && plans.length === 0 ? (
            <p className="text-xs text-text-muted">{t("rams.decision.noPlans")}</p>
          ) : null}
          {kind === "pm_interval" ? (
            <div className="grid gap-3 sm:grid-cols-2">
              <label className={field}>
                {t("rams.decision.plan")}
                <select
                  className={RAMS_SELECT_CLASS}
                  value={planId ?? ""}
                  onChange={(e) => setPlanId(e.target.value ? Number(e.target.value) : null)}
                >
                  {plans.map((p) => (
                    <option key={p.pm_plan_id} value={p.pm_plan_id}>
                      {t("rams.decision.planOption", {
                        code: p.code,
                        title: p.title,
                        hours: formatNumber(Math.round(p.interval_hours ?? 0)),
                      })}
                    </option>
                  ))}
                </select>
              </label>
              <label className={field}>
                {t("rams.decision.interval")}
                <DecimalField
                  className="h-10"
                  min={0}
                  value={interval}
                  onValueChange={setInterval}
                />
              </label>
            </div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2">
              <label className={field}>
                {t("rams.decision.equipment")}
                <select
                  className={RAMS_SELECT_CLASS}
                  value={equipmentId ?? ""}
                  onChange={(e) => setEquipmentId(e.target.value ? Number(e.target.value) : null)}
                >
                  <option value="">{t("rams.decision.chooseEquipment")}</option>
                  {study.assets.map((a) => (
                    <option key={a.equipment_id} value={a.equipment_id}>
                      {`${a.asset_code} · ${a.asset_name}`}
                    </option>
                  ))}
                </select>
              </label>
              <label className={field}>
                {t("rams.decision.woType")}
                <select
                  className={RAMS_SELECT_CLASS}
                  value={woType}
                  onChange={(e) => setWoType(e.target.value)}
                >
                  {woTypes.map((w) => (
                    <option key={w.code} value={w.code}>
                      {woTypeLabel(w, i18n.language)}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}
          <label className={field}>
            {t("rams.decision.approver")}
            <select
              className={RAMS_SELECT_CLASS}
              value={approver ?? ""}
              onChange={(e) => setApprover(e.target.value ? Number(e.target.value) : null)}
            >
              <option value="">{t("rams.decision.anyApprover")}</option>
              {/* The proposer cannot approve their own decision: they are not offered. */}
              {approvers
                .filter((a) => a.user_id !== me)
                .map((a) => (
                  <option key={a.user_id} value={a.user_id}>
                    {a.display_name}
                  </option>
                ))}
            </select>
          </label>
          <label className={field}>
            {t("rams.decision.justification")}
            <Textarea
              rows={3}
              value={justification}
              onChange={(e) => setJustification(e.target.value)}
            />
          </label>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("rams.common.cancel")}
          </Button>
          <Button className="h-11" disabled={!valid || saving} onClick={() => void submit()}>
            {t("rams.decision.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

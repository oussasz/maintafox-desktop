/**
 * Study step 4 — RCM decision (SAE JA1011 logic), one FMECA row at a time. Answers come
 * pre-filled from the FMECA row and the life analysis (each with its basis) and stay editable;
 * the recommendation is recomputed by the backend logic; the decision is recorded and sent to
 * the decision log (step 6).
 */
import { ArrowRight, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { visibleQuestions } from "@/components/reliability/RamsLoopDialogs";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_TACTIC_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type RcmQuestion,
  type RcmRow,
  type RcmWorksheet,
  type StudyRcmAnswers,
  getReliabilityStudyRcm,
  previewStudyRcm,
  saveReliabilityStudyRcmDecision,
} from "@/services/reliability-model-service";
import type { ReliabilityStudy } from "@/services/reliability-study-service";
import type { RcmAnswers } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import type { DecisionPrefill } from "./DecisionDialog";
import { roundInterval } from "./StudyLifeStep";
import { RAMS_SELECT_CLASS, RamsCallout, RamsCard } from "./rams-ui";

const TACTICS = [
  "condition_based",
  "time_based",
  "failure_finding",
  "run_to_failure",
  "redesign",
] as const;

export function StudyRcmStep({
  study,
  editable,
  onDecide,
  onGoFmeca,
}: {
  study: ReliabilityStudy;
  editable: boolean;
  onDecide: (p: DecisionPrefill) => void;
  onGoFmeca: () => void;
}) {
  const { t } = useTranslation("reliability");
  const [ws, setWs] = useState<RcmWorksheet | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<number | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const w = await getReliabilityStudyRcm(study.id);
      setWs(w);
      setSelected((cur) => cur ?? w.rows[0]?.item_id ?? null);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [study.id]);

  useEffect(() => {
    void load();
  }, [load]);

  if (error != null && ws == null)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (ws == null) return <LoadingState />;
  if (ws.rows.length === 0)
    return (
      <EmptyState
        title={t("rams.rcm.emptyTitle")}
        description={t("rams.rcm.emptyBody")}
        actionLabel={t("rams.rcm.toFmeca")}
        onAction={onGoFmeca}
      />
    );
  const row = ws.rows.find((r) => r.item_id === selected) ?? ws.rows[0];
  if (!row) return null;
  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,16rem)_minmax(0,1.4fr)_minmax(0,1fr)]">
      <RamsCard title={t("rams.rcm.modesTitle")}>
        <ul className="grid gap-1.5" role="listbox" aria-label={t("rams.rcm.modesTitle")}>
          {ws.rows.map((r) => (
            <li key={r.item_id}>
              <button
                type="button"
                role="option"
                aria-selected={r.item_id === row.item_id}
                onClick={() => setSelected(r.item_id)}
                className={cn(
                  "w-full min-h-[44px] rounded-md border px-3 py-2 text-start text-sm",
                  r.item_id === row.item_id
                    ? "border-primary bg-primary-bg text-primary"
                    : "border-surface-border hover:bg-surface-2",
                )}
              >
                <span className="block font-medium">{r.component_label || r.mode_label}</span>
                <span className="block text-xs text-text-muted">
                  {r.mode_code} · {t("rams.rcm.rpn", { rpn: r.rpn })}
                </span>
                {r.saved ? (
                  <StatusChip
                    className="mt-1"
                    tone={toneOf(RELIABILITY_TACTIC_TONE, r.saved.selected_tactic)}
                    label={t(`rams.rcm.tactic.${r.saved.selected_tactic}`)}
                  />
                ) : null}
              </button>
            </li>
          ))}
        </ul>
      </RamsCard>
      <RcmEditor
        key={row.item_id}
        study={study}
        row={row}
        timeBasis={ws.time_basis}
        editable={editable && ws.editable}
        onSaved={setWs}
        onDecide={onDecide}
      />
    </div>
  );
}

function RcmEditor({
  study,
  row,
  timeBasis,
  editable,
  onSaved,
  onDecide,
}: {
  study: ReliabilityStudy;
  row: RcmRow;
  timeBasis: string;
  editable: boolean;
  onSaved: (w: RcmWorksheet) => void;
  onDecide: (p: DecisionPrefill) => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatNumber, formatDecimal } = useFormatters();
  const { can } = usePermissions();
  const [answers, setAnswers] = useState<StudyRcmAnswers>(row.answers);
  const [pf, setPf] = useState(row.pf_interval_hours != null ? String(row.pf_interval_hours) : "");
  const [rec, setRec] = useState(row.recommendation);
  const [tactic, setTactic] = useState<string>(
    row.saved?.selected_tactic ?? row.recommendation.recommended_tactic,
  );
  const [justification, setJustification] = useState(row.saved?.justification ?? "");
  const [saving, setSaving] = useState(false);
  const pfN = Number(pf.replace(",", "."));
  const pfValid = pf.trim() === "" || (Number.isFinite(pfN) && pfN > 0);

  useEffect(() => {
    const h = setTimeout(() => {
      void previewStudyRcm(answers)
        .then((r) => {
          setRec(r);
          if (!row.saved) setTactic(r.recommended_tactic);
        })
        .catch(() => undefined);
    }, 150);
    return () => clearTimeout(h);
  }, [answers, row.saved]);

  const questions = useMemo(
    () => visibleQuestions(answers as unknown as RcmAnswers) as RcmQuestion[],
    [answers],
  );
  const basis = (q: string) => row.bases.find((b) => b.question === q);
  const unit = timeBasis === "operating" ? t("rams.units.opHours") : t("rams.units.calHours");
  const needsJustification = tactic !== rec.recommended_tactic;
  const canSave =
    editable &&
    can(P.RAM_ANALYZE) &&
    pfValid &&
    (!needsJustification || justification.trim().length > 0) &&
    !(rec.redesign_compulsory && tactic !== "redesign");

  const save = async () => {
    setSaving(true);
    try {
      onSaved(
        await saveReliabilityStudyRcmDecision({
          study_id: study.id,
          fmeca_item_id: row.item_id,
          answers,
          selected_tactic: tactic,
          justification: justification.trim() || null,
          pf_interval_hours: pf.trim() ? pfN : null,
        }),
      );
    } catch {
      // global IPC toast
    } finally {
      setSaving(false);
    }
  };

  const inspection = pf.trim() && pfValid ? pfN / 2 : null;
  const replacement =
    row.replacement_interval_hours != null ? roundInterval(row.replacement_interval_hours) : null;
  const sendToDecisions = () => {
    const saved = row.saved;
    if (!saved) return;
    const kind = saved.selected_tactic;
    const base = {
      pmPlanId: null,
      plannedCost: null,
      failureCost: null,
      fmecaItemId: row.item_id,
      rcmDecisionId: saved.decision_id,
    };
    if (kind === "time_based") {
      onDecide({
        ...base,
        kind: "pm_interval",
        intervalHours: replacement,
        title: t("rams.rcm.decisionTimeBased", {
          element: row.component_label || row.mode_label || "",
          interval: replacement != null ? formatNumber(replacement) : "…",
          unit,
        }),
      });
    } else if (kind === "redesign") {
      onDecide({
        ...base,
        kind: "modification_wo",
        intervalHours: null,
        title: t("rams.rcm.decisionRedesign", {
          element: row.component_label || row.mode_label || "",
        }),
      });
    } else {
      onDecide({
        ...base,
        kind: "one_off_wo",
        intervalHours: null,
        title:
          kind === "condition_based"
            ? t("rams.rcm.decisionCondition", {
                element: row.component_label || row.mode_label || "",
                interval: inspection != null ? formatNumber(Math.round(inspection)) : "…",
                unit,
              })
            : t("rams.rcm.decisionFailureFinding", {
                element: row.component_label || row.mode_label || "",
              }),
      });
    }
  };

  const savedMatches =
    row.saved != null &&
    row.saved.selected_tactic === tactic &&
    JSON.stringify(row.answers) === JSON.stringify(answers);

  return (
    <>
      <RamsCard
        title={t("rams.rcm.title", {
          mode: row.component_label
            ? `${row.component_label} · ${row.mode_label ?? ""}`
            : (row.mode_label ?? ""),
        })}
        subtitle={t("rams.rcm.hint")}
      >
        <div className="grid gap-1.5">
          {questions.map((q) => {
            const b = basis(q);
            return (
              <div
                key={q}
                className="grid grid-cols-[2.5rem_minmax(0,1fr)_auto] items-center gap-3 rounded-md border border-surface-border bg-surface-1 px-3 py-2"
              >
                <span className="font-mono text-xs text-text-muted">
                  {t(`rams.rcm.q.${q}.code`)}
                </span>
                <div className="min-w-0">
                  <p className="text-sm text-text-primary">{t(`rams.rcm.q.${q}.text`)}</p>
                  {b ? (
                    <p className="text-xs text-text-muted">
                      {t(`rams.rcm.basis.${b.basis}`, {
                        score: b.score ?? "",
                        value:
                          b.value != null
                            ? formatDecimal(b.value, b.basis.startsWith("weibull") ? 2 : 0)
                            : "",
                      })}
                    </p>
                  ) : null}
                </div>
                <div
                  className="inline-flex rounded-md border border-surface-border p-0.5"
                  role="group"
                  aria-label={t(`rams.rcm.q.${q}.text`)}
                >
                  {[true, false].map((v) => (
                    <button
                      key={String(v)}
                      type="button"
                      aria-pressed={answers[q] === v}
                      disabled={!editable}
                      onClick={() => setAnswers((a) => ({ ...a, [q]: v }))}
                      className={cn(
                        "min-h-9 rounded px-3 text-sm",
                        answers[q] === v
                          ? "bg-primary text-white"
                          : "text-text-secondary hover:bg-surface-2",
                      )}
                    >
                      {v ? t("rams.rcm.yes") : t("rams.rcm.no")}
                    </button>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
        <label className="mt-3 grid max-w-xs gap-1 text-xs font-medium text-text-secondary">
          {t("rams.rcm.pfInterval", { unit })}
          <Input
            inputMode="decimal"
            className="h-10"
            value={pf}
            disabled={!editable}
            aria-invalid={!pfValid}
            onChange={(e) => {
              setPf(e.target.value);
              const v = Number(e.target.value.replace(",", "."));
              if (Number.isFinite(v) && v > 0)
                setAnswers((a) => ({ ...a, on_condition_feasible: true }));
            }}
          />
        </label>
      </RamsCard>
      <RamsCard title={t("rams.rcm.recommendedTitle")}>
        <div
          className="flex flex-wrap items-center gap-1.5 text-xs"
          aria-label={t("rams.rcm.pathLabel")}
        >
          {rec.path.map((p, i) => (
            <span key={`${p}-${i}`} className="flex items-center gap-1.5">
              {i > 0 ? (
                <span aria-hidden className="text-text-muted">
                  →
                </span>
              ) : null}
              <span className="rounded-full border border-surface-border bg-surface-1 px-2.5 py-0.5">
                {t(`rams.rcm.path.${p.replace(":", "_")}`)}
              </span>
            </span>
          ))}
        </div>
        <div className="mt-3">
          <RamsCallout
            tone={toneOf(RELIABILITY_TACTIC_TONE, rec.recommended_tactic)}
            title={t(`rams.rcm.tactic.${rec.recommended_tactic}`)}
          >
            {t(`rams.rcm.tacticBody.${rec.recommended_tactic}`, {
              inspection: inspection != null ? formatNumber(Math.round(inspection)) : "…",
              replacement: replacement != null ? formatNumber(replacement) : "…",
              unit,
            })}
            {rec.redesign_compulsory ? (
              <span className="block font-medium">{t("rams.rcm.redesignCompulsory")}</span>
            ) : null}
          </RamsCallout>
        </div>
        {row.beta != null ? (
          <p className="mt-2 text-xs text-text-muted">
            {t("rams.rcm.fitNote", {
              beta: formatDecimal(row.beta, 2),
              pattern: t(`rams.life.pattern.${row.pattern ?? "random"}.title`),
            })}
          </p>
        ) : (
          <p className="mt-2 text-xs text-text-muted">{t("rams.rcm.noFitNote")}</p>
        )}
        <label className="mt-3 grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.rcm.selectedTactic")}
          <select
            className={RAMS_SELECT_CLASS}
            value={tactic}
            disabled={!editable}
            onChange={(e) => setTactic(e.target.value)}
          >
            {TACTICS.map((x) => (
              <option key={x} value={x}>
                {t(`rams.rcm.tactic.${x}`)}
              </option>
            ))}
          </select>
        </label>
        {needsJustification ? (
          <label className="mt-2 grid gap-1 text-xs font-medium text-text-secondary">
            {t("rams.rcm.justification")}
            <Textarea
              rows={2}
              value={justification}
              onChange={(e) => setJustification(e.target.value)}
            />
          </label>
        ) : null}
        <div className="mt-3 flex flex-wrap justify-end gap-2">
          {editable && can(P.RAM_ANALYZE) ? (
            <Button
              variant="outline"
              className="h-11"
              disabled={!canSave || saving}
              onClick={() => void save()}
            >
              <Save className="h-4 w-4" aria-hidden />
              {t("rams.rcm.save")}
            </Button>
          ) : null}
          {editable && can(P.RAM_ANALYZE) ? (
            <Button
              className={mfLayout.primaryAction}
              disabled={!savedMatches || row.saved?.selected_tactic === "run_to_failure"}
              title={
                !savedMatches
                  ? t("rams.rcm.saveFirst")
                  : row.saved?.selected_tactic === "run_to_failure"
                    ? t("rams.rcm.noTask")
                    : undefined
              }
              onClick={sendToDecisions}
            >
              {t("rams.rcm.toDecisions")}
              <ArrowRight className="h-4 w-4" aria-hidden />
            </Button>
          ) : null}
        </div>
        {row.decision_status ? (
          <p className="mt-2 text-xs text-text-muted">
            {t("rams.rcm.decisionRecorded", {
              status: t(`rams.decisions.status.${row.decision_status}`),
            })}
          </p>
        ) : null}
      </RamsCard>
    </>
  );
}

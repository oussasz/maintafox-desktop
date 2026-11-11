/**
 * Study step 4 — RCM decision (SAE JA1011 logic), one FMECA row at a time. No answer is ticked
 * in advance: each question shows what the FMECA row and the life analysis suggest, and waits for
 * Yes or No. The recommendation is recomputed by the backend logic once every question is
 * answered; a critical row (RPN at or above the plant threshold) is never run to failure. The
 * decision is recorded and sent to the decision log (step 6).
 */
import { ArrowRight, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { visibleQuestions } from "@/components/reliability/RamsLoopDialogs";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_TACTIC_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
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
import {
  RAMS_SELECT_CLASS,
  RamsCallout,
  RamsCard,
  RamsReadOnlyNote,
  roundInterval,
  useRamsRight,
} from "./rams-ui";

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
  // FMECA rows whose ratings nobody chose are not offered here until they are rated.
  const toReview =
    ws.rows_to_review > 0 ? (
      <RamsCallout
        tone="attention"
        title={t("rams.fmeca.rowsToReview", { count: ws.rows_to_review })}
      >
        {t("rams.rcm.rowsToReviewBody")}
      </RamsCallout>
    ) : null;
  // Ratings and consequences come from substitute grids until the plant's grids are reviewed.
  const gridsReview = ws.grids_review ? (
    <RamsCallout tone="attention" title={t("rams.scales.substituteTitle")}>
      {t("rams.scales.substituteBody")}
    </RamsCallout>
  ) : null;
  if (ws.rows.length === 0)
    return (
      <div className="grid gap-4">
        {gridsReview}
        {toReview}
        <EmptyState
          title={t("rams.rcm.emptyTitle")}
          description={t("rams.rcm.emptyBody")}
          actionLabel={t("rams.rcm.toFmeca")}
          onAction={onGoFmeca}
        />
      </div>
    );
  const row = ws.rows.find((r) => r.item_id === selected) ?? ws.rows[0];
  if (!row) return null;
  return (
    <div className="grid gap-4">
      {gridsReview}
      {toReview}
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
          rpnThreshold={ws.rpn_threshold}
          editable={editable && ws.editable}
          onSaved={setWs}
          onDecide={onDecide}
        />
      </div>
    </div>
  );
}

/** Answer suggested by the data (FMECA grids, life analysis, P-F interval); never pre-ticked. */
const DATA_BASES = new Set([
  "detection",
  "severity",
  "weibull_wear_out",
  "weibull_no_wear_out",
  "pf_interval",
]);

function RcmEditor({
  study,
  row,
  timeBasis,
  rpnThreshold,
  editable,
  onSaved,
  onDecide,
}: {
  study: ReliabilityStudy;
  row: RcmRow;
  timeBasis: string;
  rpnThreshold: number;
  editable: boolean;
  onSaved: (w: RcmWorksheet) => void;
  onDecide: (p: DecisionPrefill) => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatNumber, formatDecimal } = useFormatters();
  const { can, reasonFor } = useRamsRight();
  const [answers, setAnswers] = useState<StudyRcmAnswers>(row.answers);
  /**
   * Questions the person answered. Nothing is answered in advance (L2-15): a saved decision's
   * answers were chosen by a person; otherwise every question waits for Yes or No.
   */
  const [answered, setAnswered] = useState<ReadonlySet<RcmQuestion>>(
    () =>
      new Set(
        row.saved
          ? (Object.keys(row.answers).filter((k) => k !== "weibull_beta") as RcmQuestion[])
          : [],
      ),
  );
  const [pf, setPf] = useState<number | null>(row.pf_interval_hours ?? null);
  const [rec, setRec] = useState(row.recommendation);
  const [tactic, setTactic] = useState<string>(row.saved?.selected_tactic ?? "");
  const [justification, setJustification] = useState(row.saved?.justification ?? "");
  const [saving, setSaving] = useState(false);
  /** The recommendation could not be recomputed: the shown one is stale, saving waits. */
  const [previewError, setPreviewError] = useState<unknown>(null);
  const [previewRun, setPreviewRun] = useState(0);
  /** "On-condition feasible" was answered by typing a P-F interval (undone if it is cleared). */
  const [pfAnswered, setPfAnswered] = useState(false);
  const pfValid = pf == null || pf > 0;
  /** At or above the plant's critical RPN: never run to failure (backend refuses it too). */
  const critical = row.rpn >= rpnThreshold;

  const questions = useMemo(
    () => visibleQuestions(answers as unknown as RcmAnswers) as RcmQuestion[],
    [answers],
  );
  const missing = questions.filter((q) => !answered.has(q)).length;
  const complete = missing === 0;

  useEffect(() => {
    if (!complete) return;
    const h = setTimeout(() => {
      void previewStudyRcm(answers)
        .then((r) => {
          setRec(r);
          setPreviewError(null);
          // The recommendation becomes the tactic, unless it is refused for this mode.
          if (!row.saved)
            setTactic(
              critical && r.recommended_tactic === "run_to_failure" ? "" : r.recommended_tactic,
            );
        })
        .catch((e: unknown) => setPreviewError(e));
    }, 150);
    return () => clearTimeout(h);
  }, [answers, complete, critical, row.saved, previewRun]);

  const answer = (q: RcmQuestion, v: boolean) => {
    setAnswers((a) => ({ ...a, [q]: v }));
    setAnswered((s) => new Set(s).add(q));
  };
  const basis = (q: string) => row.bases.find((b) => b.question === q);
  const unit = timeBasis === "operating" ? t("rams.units.opHours") : t("rams.units.calHours");
  const needsJustification = tactic !== "" && tactic !== rec.recommended_tactic;
  const refusedTactic = critical && tactic === "run_to_failure";
  /** Shown to anyone on an open study; a missing right disables them with the reason. */
  const showActions = editable || (!can(P.RAM_ANALYZE) && study.status !== "archived");
  const canSave =
    editable &&
    can(P.RAM_ANALYZE) &&
    complete &&
    tactic !== "" &&
    !refusedTactic &&
    pfValid &&
    previewError == null &&
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
          pf_interval_hours: pf,
        }),
      );
    } catch {
      // global IPC toast
    } finally {
      setSaving(false);
    }
  };

  const inspection = pf != null && pfValid ? pf / 2 : null;
  const replacement =
    row.replacement_interval_hours != null ? roundInterval(row.replacement_interval_hours) : null;
  const element = row.component_label || row.mode_label || "";
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
        // No interval in the title when none is established (never a "…" left in the text).
        title:
          replacement != null
            ? t("rams.rcm.decisionTimeBased", {
                element,
                interval: formatNumber(replacement),
                unit,
              })
            : t("rams.rcm.decisionTimeBasedNoInterval", { element }),
      });
    } else if (kind === "redesign") {
      onDecide({
        ...base,
        kind: "modification_wo",
        intervalHours: null,
        title: t("rams.rcm.decisionRedesign", { element }),
      });
    } else {
      onDecide({
        ...base,
        kind: "one_off_wo",
        intervalHours: null,
        title:
          kind === "condition_based"
            ? inspection != null
              ? t("rams.rcm.decisionCondition", {
                  element,
                  interval: formatNumber(Math.round(inspection)),
                  unit,
                })
              : t("rams.rcm.decisionConditionNoInterval", { element })
            : t("rams.rcm.decisionFailureFinding", { element }),
      });
    }
  };
  /** The recommended tactic in words, without an unfilled interval. */
  const tacticBody = (x: string) => {
    if (x === "condition_based" && inspection == null)
      return t("rams.rcm.tacticBody.condition_based_noInterval");
    if (x === "time_based" && replacement == null)
      return t("rams.rcm.tacticBody.time_based_noInterval");
    return t(`rams.rcm.tacticBody.${x}`, {
      inspection: inspection != null ? formatNumber(Math.round(inspection)) : "",
      replacement: replacement != null ? formatNumber(replacement) : "",
      unit,
    });
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
            const isAnswered = answered.has(q);
            return (
              <div
                key={q}
                className="grid grid-cols-[minmax(0,10rem)_minmax(0,1fr)_auto] items-center gap-3 rounded-md border border-surface-border bg-surface-1 px-3 py-2"
              >
                <span className="text-xs font-medium text-text-secondary">
                  {t(`rams.rcm.q.${q}.label`)}
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
                      {DATA_BASES.has(b.basis) && !isAnswered
                        ? ` ${t("rams.rcm.suggested", {
                            answer: row.prefilled[q] ? t("rams.rcm.yes") : t("rams.rcm.no"),
                          })}`
                        : ""}
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
                      aria-pressed={isAnswered && answers[q] === v}
                      disabled={!editable}
                      onClick={() => answer(q, v)}
                      className={cn(
                        "min-h-9 rounded px-3 text-sm",
                        isAnswered && answers[q] === v
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
          <DecimalField
            className="h-10"
            min={0}
            value={pf}
            disabled={!editable}
            aria-invalid={pfValid ? undefined : true}
            onValueChange={(v) => {
              setPf(v);
              const known = v != null && v > 0;
              // A known P-F interval answers "monitoring possible"; clearing it gives the question
              // back to the analyst only when the interval had answered it.
              if (
                known &&
                !(answered.has("on_condition_feasible") && answers.on_condition_feasible)
              ) {
                setPfAnswered(true);
                answer("on_condition_feasible", true);
              } else if (!known && pfAnswered) {
                setPfAnswered(false);
                setAnswers((x) => ({ ...x, on_condition_feasible: false }));
                setAnswered((s) => {
                  const next = new Set(s);
                  next.delete("on_condition_feasible");
                  return next;
                });
              }
            }}
          />
        </label>
      </RamsCard>
      <RamsCard title={t("rams.rcm.recommendedTitle")}>
        {previewError != null ? (
          <div className="mb-3">
            <RamsCallout
              tone="danger"
              title={t("rams.rcm.previewError")}
              action={
                <Button
                  variant="outline"
                  className="h-10"
                  onClick={() => setPreviewRun((n) => n + 1)}
                >
                  {t("rams.rcm.previewRetry")}
                </Button>
              }
            >
              {describeAppError(previewError).title}
            </RamsCallout>
          </div>
        ) : null}
        {!complete ? (
          <p className="text-sm text-text-muted" role="status">
            {t("rams.rcm.answerAll", { count: missing })}
          </p>
        ) : (
          <>
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
                {tacticBody(rec.recommended_tactic)}
                {rec.redesign_compulsory ? (
                  <span className="block font-medium">{t("rams.rcm.redesignCompulsory")}</span>
                ) : null}
              </RamsCallout>
            </div>
          </>
        )}
        {critical &&
        (refusedTactic || (complete && rec.recommended_tactic === "run_to_failure")) ? (
          <div className="mt-3">
            <RamsCallout
              tone="danger"
              title={t("rams.rcm.criticalTitle", { rpn: row.rpn, threshold: rpnThreshold })}
            >
              {t("rams.rcm.criticalBody")}
            </RamsCallout>
          </div>
        ) : null}
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
            disabled={!editable || !complete}
            onChange={(e) => setTactic(e.target.value)}
          >
            <option value="" disabled>
              {t("rams.decision.chooseEquipment")}
            </option>
            {TACTICS.map((x) => (
              <option key={x} value={x} disabled={critical && x === "run_to_failure"}>
                {critical && x === "run_to_failure"
                  ? t("rams.rcm.refusedOption", { tactic: t(`rams.rcm.tactic.${x}`) })
                  : t(`rams.rcm.tactic.${x}`)}
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
        {!can(P.RAM_ANALYZE) && showActions ? (
          <div className="mt-2">
            <RamsReadOnlyNote right={P.RAM_ANALYZE} />
          </div>
        ) : null}
        <div className="mt-3 flex flex-wrap justify-end gap-2">
          {showActions ? (
            <Button
              variant="outline"
              className="h-11"
              disabled={!canSave || saving}
              title={reasonFor(P.RAM_ANALYZE)}
              onClick={() => void save()}
            >
              <Save className="h-4 w-4" aria-hidden />
              {t("rams.rcm.save")}
            </Button>
          ) : null}
          {showActions ? (
            <Button
              className={mfLayout.primaryAction}
              disabled={
                !can(P.RAM_ANALYZE) ||
                !savedMatches ||
                row.saved?.selected_tactic === "run_to_failure"
              }
              title={
                reasonFor(P.RAM_ANALYZE) ??
                (!savedMatches
                  ? t("rams.rcm.saveFirst")
                  : row.saved?.selected_tactic === "run_to_failure"
                    ? t("rams.rcm.noTask")
                    : undefined)
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

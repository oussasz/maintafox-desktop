/**
 * Reliability study workspace: scope bar (equipment, period, failure mode, time base) and the six
 * steps — data, life (Weibull), FMECA, RCM decision, system model, decisions.
 */
import { ArrowLeft, ClipboardCheck } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import {
  DecisionDialog,
  type DecisionPrefill,
} from "@/components/reliability/study/DecisionDialog";
import { StudyDataStep } from "@/components/reliability/study/StudyDataStep";
import { StudyDecisionsStep } from "@/components/reliability/study/StudyDecisionsStep";
import { StudyFmecaStep } from "@/components/reliability/study/StudyFmecaStep";
import { StudyLifeStep } from "@/components/reliability/study/StudyLifeStep";
import { StudyRcmStep } from "@/components/reliability/study/StudyRcmStep";
import { StudySystemStep } from "@/components/reliability/study/StudySystemStep";
import {
  RAMS_FILTER_LABEL,
  RAMS_SELECT_CLASS,
  dayToIso,
  isoDay,
} from "@/components/reliability/study/rams-ui";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { listFailureCodes, listFailureHierarchies } from "@/services/reliability-service";
import {
  type StudyDataset,
  type UpdateStudyInput,
  getReliabilityStudyDataset,
  updateReliabilityStudy,
} from "@/services/reliability-study-service";
import type { FailureCode } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export const STUDY_STEPS = ["data", "life", "fmeca", "rcm", "system", "decisions"] as const;
export type StudyStep = (typeof STUDY_STEPS)[number];

/**
 * A period date of the study: typed or picked, then saved when the field is left. `onCommit`
 * returns false to refuse the date; the field then shows the saved one again.
 */
function StudyDateField({
  saved,
  disabled,
  label,
  invalid,
  describedBy,
  onCommit,
}: {
  saved: string;
  disabled: boolean;
  label: string;
  invalid: boolean;
  describedBy: string | undefined;
  onCommit: (value: string) => boolean;
}) {
  const [value, setValue] = useState(saved);
  // Remounting the field is how a refused date goes back to the saved one on screen.
  const [resets, setResets] = useState(0);
  const committed = useRef(saved);
  useEffect(() => {
    committed.current = saved;
    setValue(saved);
  }, [saved]);

  const refuse = () => {
    setValue(saved);
    setResets((n) => n + 1);
  };
  /** Saves a finished date once; a date already sent (typed, then field left) is not sent twice. */
  const commit = (v: string) => {
    if (!v || v === committed.current) return;
    if (onCommit(v)) committed.current = v;
    else refuse();
  };

  return (
    <DateField
      key={resets}
      className={cn(RAMS_SELECT_CLASS, "min-w-0")}
      value={value}
      onChange={(v) => {
        setValue(v);
        // A finished date (typed in full or picked in the calendar) is saved at once; a short
        // year is read when the field is left.
        commit(v);
      }}
      disabled={disabled}
      aria-label={label}
      aria-invalid={invalid || undefined}
      aria-describedby={describedBy}
      onBlur={() => {
        if (!value) refuse();
        else commit(value);
      }}
    />
  );
}

export function RamsStudyPage() {
  const { t } = useTranslation("reliability");
  const navigate = useNavigate();
  const { studyId: raw } = useParams();
  const studyId = Number(raw);
  const [params, setParams] = useSearchParams();
  const stepParam = params.get("step");
  const step: StudyStep = (STUDY_STEPS as readonly string[]).includes(stepParam ?? "")
    ? (stepParam as StudyStep)
    : "data";
  const { can } = usePermissions();
  const canAnalyze = can(P.RAM_ANALYZE);
  const [dataset, setDataset] = useState<StudyDataset | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [modes, setModes] = useState<FailureCode[]>([]);
  const [decisionOpen, setDecisionOpen] = useState(false);
  const [prefill, setPrefill] = useState<DecisionPrefill | null>(null);
  const [decisionsVersion, setDecisionsVersion] = useState(0);
  const [saving, setSaving] = useState(false);
  const [dateError, setDateError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!Number.isFinite(studyId) || studyId <= 0) return;
    setLoading(true);
    setError(null);
    try {
      setDataset(await getReliabilityStudyDataset(studyId));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [studyId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const hierarchies = await listFailureHierarchies();
        const all: FailureCode[] = [];
        for (const h of hierarchies.filter((x) => x.is_active)) {
          const codes = await listFailureCodes({ hierarchy_id: h.id, include_inactive: false });
          all.push(...codes.filter((c) => c.code_type === "mode"));
        }
        if (!cancelled) setModes(all);
      } catch {
        if (!cancelled) setModes([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const goStep = (s: StudyStep) =>
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set("step", s);
        return next;
      },
      { replace: true },
    );

  const study = dataset?.study;
  const updateScope = async (patch: Omit<UpdateStudyInput, "id" | "expected_row_version">) => {
    if (!study) return;
    setSaving(true);
    try {
      await updateReliabilityStudy({
        id: study.id,
        expected_row_version: study.row_version,
        ...patch,
      });
      await load();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  const openDecision = (p: DecisionPrefill | null) => {
    setPrefill(p);
    setDecisionOpen(true);
  };

  if (!Number.isFinite(studyId) || studyId <= 0) {
    return (
      <ModulePageShell title={t("rams.title")}>
        <ErrorState error={t("rams.study.notFound")} />
      </ModulePageShell>
    );
  }

  const toCheck = dataset?.checks.filter((c) => c.severity === "warning").length ?? 0;
  const archived = study?.status === "archived";
  const editable = canAnalyze && !archived;

  return (
    <ModulePageShell
      headerLeft={
        <div className="flex min-w-0 items-center gap-3">
          <Button variant="outline" className="h-11" onClick={() => navigate("/reliability")}>
            <ArrowLeft className="h-4 w-4" aria-hidden />
            {t("rams.study.back")}
          </Button>
          <h1 className={cn(mfLayout.moduleTitle, "truncate")}>
            {study ? t("rams.study.title", { title: study.title }) : t("rams.title")}
          </h1>
          {archived ? <StatusChip tone="neutral" label={t("rams.study.archived")} /> : null}
        </div>
      }
      actions={
        study && !archived ? (
          <PermissionGate permission={P.RAM_ANALYZE}>
            <Button className={mfLayout.primaryAction} onClick={() => openDecision(null)}>
              <ClipboardCheck className="h-4 w-4" aria-hidden />
              {t("rams.study.recordDecision")}
            </Button>
          </PermissionGate>
        ) : null
      }
      bodyClassName={null}
      filterBar={
        study ? (
          <div className={cn(mfLayout.moduleFilterBar, "gap-3")}>
            <span className={RAMS_FILTER_LABEL}>{t("rams.study.scope")}</span>
            <span className="text-sm font-medium text-text-primary">
              {study.assets.map((a) => a.asset_code).join(", ")}
            </span>
            <label className="flex items-center gap-1.5">
              <span className="sr-only">{t("rams.study.periodStart")}</span>
              <StudyDateField
                key={`s-${study.row_version}`}
                saved={isoDay(study.period_start)}
                disabled={!editable || saving}
                label={t("rams.study.periodStart")}
                invalid={dateError != null}
                describedBy={dateError ? "rams-period-error" : undefined}
                onCommit={(v) => {
                  // The period must start before it ends: refused here, not saved.
                  if (v > isoDay(study.period_end)) {
                    setDateError(t("rams.study.periodOrder"));
                    return false;
                  }
                  setDateError(null);
                  void updateScope({ period_start: dayToIso(v, false) });
                  return true;
                }}
              />
              <span aria-hidden>–</span>
              <StudyDateField
                key={`e-${study.row_version}`}
                saved={isoDay(study.period_end)}
                disabled={!editable || saving}
                label={t("rams.study.periodEnd")}
                invalid={dateError != null}
                describedBy={dateError ? "rams-period-error" : undefined}
                onCommit={(v) => {
                  if (v < isoDay(study.period_start)) {
                    setDateError(t("rams.study.periodOrder"));
                    return false;
                  }
                  setDateError(null);
                  void updateScope({ period_end: dayToIso(v, true) });
                  return true;
                }}
              />
            </label>
            {dateError ? (
              <p id="rams-period-error" role="alert" className="text-xs text-tone-danger-fg">
                {dateError}
              </p>
            ) : null}
            <label className="flex items-center gap-1.5">
              <span className="sr-only">{t("rams.study.failureMode")}</span>
              <select
                className={RAMS_SELECT_CLASS}
                value={study.failure_mode_id ?? ""}
                disabled={!editable || saving}
                aria-label={t("rams.study.failureMode")}
                onChange={(e) =>
                  void updateScope(
                    e.target.value
                      ? { failure_mode_id: Number(e.target.value) }
                      : { clear_failure_mode: true },
                  )
                }
              >
                <option value="">{t("rams.study.allModes")}</option>
                {modes.map((m) => (
                  <option key={m.id} value={m.id}>
                    {`${m.code} · ${m.label}`}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1.5">
              <span className="sr-only">{t("rams.study.timeBasis")}</span>
              <select
                className={RAMS_SELECT_CLASS}
                value={study.time_basis}
                disabled={!editable || saving}
                aria-label={t("rams.study.timeBasis")}
                onChange={(e) =>
                  void updateScope({ time_basis: e.target.value as "auto" | "calendar" })
                }
              >
                <option value="auto">{t("rams.study.basisAuto")}</option>
                <option value="calendar">{t("rams.study.basisCalendar")}</option>
              </select>
            </label>
            <label className="flex items-center gap-1.5">
              <span className="sr-only">{t("rams.study.competingModes")}</span>
              <select
                className={RAMS_SELECT_CLASS}
                value={study.competing_modes}
                disabled={!editable || saving || study.failure_mode_id == null}
                title={
                  study.failure_mode_id == null ? t("rams.study.competingNeedsMode") : undefined
                }
                aria-label={t("rams.study.competingModes")}
                onChange={(e) =>
                  void updateScope({ competing_modes: e.target.value as "ignore" | "suspend" })
                }
              >
                <option value="ignore">{t("rams.study.competingIgnore")}</option>
                <option value="suspend">{t("rams.study.competingSuspend")}</option>
              </select>
            </label>
            <StatusChip
              tone={dataset?.effective_time_basis === "operating" ? "success" : "warning"}
              label={
                dataset?.effective_time_basis === "operating"
                  ? t("rams.study.basisUsedOperating")
                  : t("rams.study.basisUsedCalendar")
              }
            />
            {toCheck > 0 ? (
              <StatusChip tone="warning" dot label={t("rams.study.toCheck", { count: toCheck })} />
            ) : null}
          </div>
        ) : null
      }
    >
      {error != null && dataset == null ? (
        <div className="p-4">
          <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
        </div>
      ) : dataset == null ? (
        <LoadingState />
      ) : (
        <>
          <div
            role="tablist"
            aria-label={t("rams.study.stepsLabel")}
            className="flex gap-0.5 overflow-x-auto border-b border-surface-border bg-surface-1 px-6"
          >
            {STUDY_STEPS.map((s, i) => (
              <button
                key={s}
                type="button"
                role="tab"
                id={`rams-tab-${s}`}
                aria-selected={step === s}
                aria-controls={`rams-panel-${s}`}
                tabIndex={step === s ? 0 : -1}
                onClick={() => goStep(s)}
                onKeyDown={(e) => {
                  // Tabs pattern: arrows move between steps, Home / End to the first / last.
                  const last = STUDY_STEPS.length - 1;
                  const to =
                    e.key === "ArrowRight"
                      ? Math.min(i + 1, last)
                      : e.key === "ArrowLeft"
                        ? Math.max(i - 1, 0)
                        : e.key === "Home"
                          ? 0
                          : e.key === "End"
                            ? last
                            : null;
                  if (to == null) return;
                  e.preventDefault();
                  const next = STUDY_STEPS[to];
                  if (next) {
                    goStep(next);
                    document.getElementById(`rams-tab-${next}`)?.focus();
                  }
                }}
                className={cn(
                  "inline-flex min-h-[44px] items-center gap-1.5 whitespace-nowrap border-b-2 px-3 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  step === s
                    ? "border-primary font-semibold text-primary"
                    : "border-transparent text-text-secondary hover:text-text-primary",
                )}
              >
                <span
                  aria-hidden
                  className={cn(
                    "inline-flex h-5 w-5 items-center justify-center rounded-full text-[11px] font-semibold",
                    step === s ? "bg-primary text-white" : "bg-surface-2 text-text-secondary",
                  )}
                >
                  {i + 1}
                </span>
                {t(`rams.steps.${s}`)}
              </button>
            ))}
          </div>
          <div
            role="tabpanel"
            id={`rams-panel-${step}`}
            aria-labelledby={`rams-tab-${step}`}
            className="p-4"
          >
            {step === "data" ? (
              <StudyDataStep
                dataset={dataset}
                editable={editable}
                onChanged={setDataset}
                onNext={() => goStep("life")}
              />
            ) : null}
            {step === "life" ? (
              <StudyLifeStep
                study={dataset.study}
                editable={editable}
                datasetVersion={
                  dataset.study.updated_at + dataset.failures_used + dataset.suspensions_used
                }
                onStudyChanged={() => void load()}
                onDecide={(p) => openDecision(p)}
              />
            ) : null}
            {step === "fmeca" ? (
              <StudyFmecaStep
                study={dataset.study}
                editable={editable}
                onGoRcm={() => goStep("rcm")}
              />
            ) : null}
            {step === "rcm" ? (
              <StudyRcmStep
                study={dataset.study}
                editable={editable}
                onDecide={(p) => openDecision(p)}
                onGoFmeca={() => goStep("fmeca")}
              />
            ) : null}
            {step === "system" ? (
              <StudySystemStep study={dataset.study} editable={editable} />
            ) : null}
            {step === "decisions" ? (
              <StudyDecisionsStep
                study={dataset.study}
                version={decisionsVersion}
                onRecord={() => openDecision(null)}
              />
            ) : null}
          </div>
        </>
      )}
      {study ? (
        <DecisionDialog
          open={decisionOpen}
          onOpenChange={setDecisionOpen}
          study={study}
          prefill={prefill}
          onCreated={() => {
            setDecisionsVersion((v) => v + 1);
            void load();
            goStep("decisions");
          }}
        />
      ) : null}
    </ModulePageShell>
  );
}

/**
 * Study step 2 — life analysis: MLE Weibull fit with suspensions and 90 % bounds, B10, mean
 * life, fit quality ρ, plain reading of β, probability plot, R(t)/h(t), and the cost-based
 * replacement interval with the management decision note.
 */
import { ClipboardCheck, Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_PATTERN_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import {
  type LifeAnalysis,
  type ReliabilityStudy,
  getReliabilityStudyLifeAnalysis,
  saveReliabilityStudyCosts,
} from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

import type { DecisionPrefill } from "./DecisionDialog";
import { CostRateChart, ReliabilityHazardChart, WeibullProbabilityPlot } from "./RamsCharts";
import { RAMS_SELECT_CLASS, RAMS_TABLE_FRAME, RamsCallout, RamsCard } from "./rams-ui";

/** Round an interval for display and for the decision (hundreds of hours above 1 000 h). */
export function roundInterval(h: number): number {
  if (h >= 1000) return Math.round(h / 100) * 100;
  if (h >= 100) return Math.round(h / 10) * 10;
  return Math.max(1, Math.round(h));
}

/** A cost counts only when it is a positive amount. */
function positiveAmount(v: number | null): number | null {
  return v != null && Number.isFinite(v) && v > 0 ? v : null;
}

export function StudyLifeStep({
  study,
  editable,
  datasetVersion,
  onStudyChanged,
  onDecide,
}: {
  study: ReliabilityStudy;
  editable: boolean;
  datasetVersion: string;
  onStudyChanged: () => void;
  onDecide: (p: DecisionPrefill) => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal, formatNumber, formatCurrency, currencyCode } = useFormatters();
  const { can } = usePermissions();
  const [analysis, setAnalysis] = useState<LifeAnalysis | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [planned, setPlanned] = useState<number | null>(null);
  const [failure, setFailure] = useState<number | null>(null);
  const [planId, setPlanId] = useState<number | null>(null);
  const [asDefault, setAsDefault] = useState(false);
  const [saving, setSaving] = useState(false);

  const plannedN = positiveAmount(planned);
  const failureN = positiveAmount(failure);

  const load = useCallback(
    async (
      costs: { planned: number | null; failure: number | null } | null,
      plan: number | null,
    ) => {
      setLoading(true);
      setError(null);
      try {
        const a = await getReliabilityStudyLifeAnalysis({
          study_id: study.id,
          planned_cost: costs?.planned ?? null,
          failure_cost: costs?.failure ?? null,
          pm_plan_id: plan,
        });
        setAnalysis(a);
        if (costs == null && a.cost) {
          setPlanned(a.cost.planned_cost);
          setFailure(a.cost.failure_cost);
        }
        setPlanId(a.selected_pm_plan_id);
      } catch (e) {
        setError(e);
      } finally {
        setLoading(false);
      }
    },
    [study.id],
  );

  useEffect(() => {
    void load(null, null);
    // Reload when the records change (step 1) or the scope changes.
  }, [load, datasetVersion]);

  // Recompute while costs are typed (no write).
  useEffect(() => {
    if (plannedN == null || failureN == null) return;
    if (
      analysis?.cost &&
      analysis.cost.planned_cost === plannedN &&
      analysis.cost.failure_cost === failureN
    )
      return;
    const h = setTimeout(() => void load({ planned: plannedN, failure: failureN }, planId), 350);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plannedN, failureN]);

  const fmt = (v: number, d = 0) => formatDecimal(v, d);
  const hours = (v: number | null | undefined) => (v == null ? "—" : formatNumber(Math.round(v)));

  if (error != null && analysis == null) {
    return <ErrorState error={error} onRetry={() => void load(null, planId)} retrying={loading} />;
  }
  if (analysis == null) return <LoadingState />;

  const fit = analysis.fit;
  const cost = analysis.cost;
  const plan = analysis.pm_plans.find((p) => p.pm_plan_id === analysis.selected_pm_plan_id) ?? null;
  const currentInterval = plan?.interval_hours ?? null;
  const unit =
    analysis.effective_time_basis === "operating"
      ? t("rams.units.opHours")
      : t("rams.units.calHours");
  const proposed = cost?.proposed?.interval != null ? roundInterval(cost.proposed.interval) : null;
  const currency = analysis.cost_currency_code ?? currencyCode;
  const money = (v: number | null | undefined) => (v == null ? "—" : formatCurrency(v, currency));
  const shareClass = analysis.cost_class_id;

  const saveCosts = async () => {
    if (plannedN == null || failureN == null) return;
    setSaving(true);
    try {
      await saveReliabilityStudyCosts({
        study_id: study.id,
        expected_row_version: study.row_version,
        planned_cost: plannedN,
        failure_cost: failureN,
        currency_code: currencyCode,
        save_as_class_default_for: asDefault ? shareClass : null,
      });
      onStudyChanged();
    } catch {
      // global IPC toast
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="grid gap-4">
      {error != null ? (
        // A recompute (costs typed, plan changed) failed: the figures below are the last ones.
        <RamsCallout
          tone="danger"
          title={t("rams.life.recomputeError")}
          action={
            <Button variant="outline" className="h-10" onClick={() => void load(null, planId)}>
              {t("rams.life.recomputeRetry")}
            </Button>
          }
        >
          {describeAppError(error).title}
        </RamsCallout>
      ) : null}
      {fit == null ? (
        <RamsCallout
          tone="danger"
          title={t(`rams.life.issue.${analysis.fit_issue ?? "not_enough_failures"}.title`)}
        >
          {t(`rams.life.issue.${analysis.fit_issue ?? "not_enough_failures"}.body`, {
            min: analysis.min_failures,
            count: analysis.n_failures,
          })}
        </RamsCallout>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(160px,1fr))] gap-3">
          <KpiTile
            label={t("rams.life.beta")}
            value={fmt(fit.beta, 2)}
            hint={t("rams.life.bounds", { lo: fmt(fit.beta_lower, 2), hi: fmt(fit.beta_upper, 2) })}
          />
          <KpiTile
            label={t("rams.life.eta")}
            value={hours(fit.eta)}
            unit="h"
            hint={t("rams.life.bounds", { lo: hours(fit.eta_lower), hi: hours(fit.eta_upper) })}
          />
          <KpiTile
            label={t("rams.life.b10")}
            value={hours(fit.b10)}
            unit="h"
            hint={
              fit.b10_lower != null && fit.b10_upper != null
                ? t("rams.life.bounds", { lo: hours(fit.b10_lower), hi: hours(fit.b10_upper) })
                : t("rams.life.b10Hint")
            }
          />
          <KpiTile
            label={t("rams.life.mttf")}
            value={hours(fit.mean_life)}
            unit="h"
            hint={t("rams.life.mttfHint")}
          />
          <KpiTile
            label={t("rams.life.data")}
            value={formatNumber(analysis.n_failures)}
            unit={t("rams.life.failuresUnit")}
            hint={t("rams.life.suspensions", { count: analysis.n_suspensions })}
          />
          <KpiTile
            label={t("rams.life.rho")}
            value={fit.rho == null ? "—" : fmt(fit.rho, 3)}
            {...(fit.rho != null && fit.rho < 0.9 ? { tone: "warning" as const } : {})}
            hint={
              fit.rho != null && fit.rho >= 0.95 ? t("rams.life.rhoGood") : t("rams.life.rhoCheck")
            }
          />
        </div>
      )}

      {fit ? (
        <div className="grid gap-4 xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
          <RamsCard title={t("rams.life.plotTitle")} subtitle={t("rams.life.plotHint")}>
            <WeibullProbabilityPlot
              points={analysis.plot_points}
              band={analysis.band}
              eta={fit.eta}
              formatNumber={fmt}
              labels={{
                title: t("rams.life.plotTitle"),
                xAxis: t("rams.life.plotX", { unit }),
                yAxis: t("rams.life.plotY"),
                fitted: t("rams.life.fitted"),
                failures: t("rams.life.failuresLegend"),
                band: t("rams.life.bandLegend"),
                eta: t("rams.life.etaMarker", { value: hours(fit.eta) }),
                pointTitle: (time, rank) => t("rams.life.pointTitle", { time, rank }),
              }}
            />
          </RamsCard>
          <div className="grid content-start gap-4">
            <RamsCard title={t("rams.life.readingTitle")}>
              <RamsCallout
                tone={toneOf(RELIABILITY_PATTERN_TONE, fit.pattern)}
                title={t(`rams.life.pattern.${fit.pattern}.title`)}
              >
                {t(`rams.life.pattern.${fit.pattern}.body`)}
              </RamsCallout>
              <p className="mt-2 text-sm text-text-secondary">
                {t("rams.life.median", { median: hours(fit.median_life), unit })}{" "}
                {plan && currentInterval != null
                  ? t(
                      currentInterval > fit.median_life
                        ? "rams.life.pmAfterMedian"
                        : "rams.life.pmBeforeMedian",
                      {
                        plan: plan.code,
                        interval: hours(currentInterval),
                      },
                    )
                  : t("rams.life.noPm")}
              </p>
            </RamsCard>
            <RamsCard title={t("rams.life.methodTitle")}>
              <ul className="grid list-disc gap-1 ps-4 text-xs text-text-secondary">
                <li>{t("rams.life.method.mle")}</li>
                <li>
                  {t(
                    analysis.effective_time_basis === "operating"
                      ? "rams.life.method.operating"
                      : "rams.life.method.calendar",
                  )}
                </li>
                <li>{t("rams.life.method.renewal")}</li>
                {analysis.asset_count > 1 ? (
                  <li>{t("rams.life.method.pooled", { count: analysis.asset_count })}</li>
                ) : null}
                <li>{t("rams.life.method.bounds")}</li>
                {analysis.n_failures < 10 ? <li>{t("rams.life.method.small")}</li> : null}
              </ul>
            </RamsCard>
          </div>
        </div>
      ) : null}

      {fit ? (
        <div className="grid gap-4 xl:grid-cols-2">
          <RamsCard title={t("rams.life.curveTitle")} subtitle={t("rams.life.curveHint")}>
            <ReliabilityHazardChart
              curve={analysis.curve}
              proposedInterval={proposed}
              currentInterval={currentInterval}
              formatNumber={fmt}
              labels={{
                title: t("rams.life.curveTitle"),
                xAxis: t("rams.life.ageAxis", { unit }),
                reliability: t("rams.life.reliabilityLegend"),
                hazard: t("rams.life.hazardLegend"),
                hazardUnit: t("rams.life.hazardUnit"),
                proposed: t("rams.life.proposedLegend"),
                current: t("rams.life.currentLegend"),
              }}
            />
          </RamsCard>
          <RamsCard title={t("rams.cost.title")} subtitle={t("rams.cost.hint")}>
            <div className="grid gap-3 sm:grid-cols-2">
              <label className="grid gap-1 text-xs font-medium text-text-secondary">
                {t("rams.cost.planned", { currency })}
                <DecimalField
                  className="h-10"
                  min={0}
                  value={planned}
                  disabled={!editable}
                  onValueChange={setPlanned}
                />
              </label>
              <label className="grid gap-1 text-xs font-medium text-text-secondary">
                {t("rams.cost.failure", { currency })}
                <DecimalField
                  className="h-10"
                  min={0}
                  value={failure}
                  disabled={!editable}
                  onValueChange={setFailure}
                />
              </label>
              <label className="grid gap-1 text-xs font-medium text-text-secondary sm:col-span-2">
                {t("rams.cost.pmPlan")}
                <select
                  className={RAMS_SELECT_CLASS}
                  value={planId ?? ""}
                  onChange={(e) => {
                    const id = e.target.value ? Number(e.target.value) : null;
                    setPlanId(id);
                    void load(
                      plannedN != null && failureN != null
                        ? { planned: plannedN, failure: failureN }
                        : null,
                      id,
                    );
                  }}
                >
                  <option value="">{t("rams.cost.noPlan")}</option>
                  {analysis.pm_plans.map((p) => (
                    <option
                      key={p.pm_plan_id}
                      value={p.pm_plan_id}
                      disabled={p.interval_hours == null}
                    >
                      {p.interval_hours != null
                        ? t("rams.cost.planOption", {
                            code: p.code,
                            title: p.title,
                            asset: p.asset_code,
                            hours: hours(p.interval_hours),
                          })
                        : t("rams.cost.planOptionNoInterval", {
                            code: p.code,
                            title: p.title,
                            reason: t(
                              `rams.cost.intervalNote.${p.interval_note ?? "invalid_trigger"}`,
                            ),
                          })}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <p className="mt-2 text-xs text-text-muted">
              {t(`rams.cost.source.${analysis.cost_source}`, {
                className: analysis.cost_class_name ?? "",
              })}
            </p>
            {editable ? (
              <div className="mt-2 flex flex-wrap items-center gap-3">
                {shareClass != null && can(P.RAM_MANAGE) ? (
                  <label className="inline-flex min-h-[36px] items-center gap-2 text-sm text-text-secondary">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-primary"
                      checked={asDefault}
                      onChange={(e) => setAsDefault(e.target.checked)}
                    />
                    {t("rams.cost.asDefault", { className: analysis.cost_class_name ?? "" })}
                  </label>
                ) : null}
                <Button
                  variant="outline"
                  className="h-10"
                  disabled={plannedN == null || failureN == null || saving}
                  onClick={() => void saveCosts()}
                >
                  <Save className="h-4 w-4" aria-hidden />
                  {t("rams.cost.save")}
                </Button>
              </div>
            ) : null}
            {cost ? (
              <div className="mt-3">
                <CostRateChart
                  curve={cost.curve}
                  runToFailureRate={cost.run_to_failure.cost_rate}
                  currentInterval={currentInterval}
                  proposed={
                    cost.proposed?.interval != null
                      ? { interval: cost.proposed.interval, cost_rate: cost.proposed.cost_rate }
                      : null
                  }
                  formatNumber={fmt}
                  labels={{
                    title: t("rams.cost.chartTitle"),
                    xAxis: t("rams.cost.chartX", { unit }),
                    yAxis: t("rams.cost.chartY", { currency }),
                    costRate: t("rams.cost.costRateLegend"),
                    runToFailure: t("rams.cost.rtfLegend"),
                    current:
                      currentInterval != null
                        ? t("rams.cost.currentLegend", { hours: hours(currentInterval) })
                        : "",
                    proposed: t("rams.cost.proposedLegend"),
                  }}
                />
              </div>
            ) : (
              <p className="mt-3 text-sm text-text-muted">{t("rams.cost.enterCosts")}</p>
            )}
          </RamsCard>
        </div>
      ) : null}

      {fit && cost ? (
        <section className="rounded-lg border border-surface-border border-s-4 border-s-primary bg-surface-1 p-4">
          <h4 className="text-sm font-semibold text-text-primary">{t("rams.memo.title")}</h4>
          {cost.proposed?.interval != null && proposed != null ? (
            <p className="mt-1 text-sm text-text-secondary">
              {t(currentInterval != null ? "rams.memo.proposeVsCurrent" : "rams.memo.propose", {
                assets: study.assets.map((a) => a.asset_code).join(", "),
                interval: formatNumber(proposed),
                current: hours(currentInterval),
                unit,
              })}{" "}
              {t("rams.memo.assumptions", {
                planned: money(cost.planned_cost),
                failure: money(cost.failure_cost),
                hpy: hours(analysis.operating_hours_per_year),
                unit,
                count: analysis.asset_count,
              })}
            </p>
          ) : (
            <p className="mt-1 text-sm text-text-secondary">
              {t(`rams.memo.noInterval.${cost.rejection ?? "no_benefit"}`)}
            </p>
          )}
          <div className={`${RAMS_TABLE_FRAME} mt-3`}>
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                    <th className="px-3 py-2 text-start">{t("rams.memo.colStrategy")}</th>
                    <th className="px-3 py-2 text-end">{t("rams.memo.colRate", { unit })}</th>
                    <th className="px-3 py-2 text-end">
                      {t("rams.memo.colAnnual", { count: analysis.asset_count })}
                    </th>
                    <th className="px-3 py-2 text-end">{t("rams.memo.colDelta")}</th>
                  </tr>
                </thead>
                <tbody>
                  {[
                    { key: "rtf", label: t("rams.memo.rtf"), s: cost.run_to_failure },
                    cost.current
                      ? {
                          key: "current",
                          label: t("rams.memo.current", { interval: hours(cost.current.interval) }),
                          s: cost.current,
                        }
                      : null,
                    cost.proposed && proposed != null
                      ? {
                          key: "proposed",
                          label: t("rams.memo.proposed", { interval: formatNumber(proposed) }),
                          s: cost.proposed,
                        }
                      : null,
                  ]
                    .filter((r): r is NonNullable<typeof r> => r != null)
                    .map((r) => {
                      const delta =
                        cost.current?.annual_cost != null &&
                        r.s.annual_cost != null &&
                        r.key !== "current"
                          ? r.s.annual_cost - cost.current.annual_cost
                          : null;
                      return (
                        <tr key={r.key} className="h-9 border-t border-surface-border">
                          <td className={`px-3 ${r.key === "proposed" ? "font-semibold" : ""}`}>
                            {r.label}
                          </td>
                          <td className="px-3 text-end font-mono tabular-nums">
                            {money(r.s.cost_rate)}
                          </td>
                          <td className="px-3 text-end font-mono tabular-nums">
                            {money(r.s.annual_cost)}
                          </td>
                          <td className="px-3 text-end font-mono tabular-nums">
                            {delta == null ? "—" : `${delta > 0 ? "+" : ""}${money(delta)}`}
                          </td>
                        </tr>
                      );
                    })}
                </tbody>
              </table>
            </div>
          </div>
          {editable && proposed != null ? (
            <div className="mt-3 flex justify-end">
              <Button
                className={mfLayout.primaryAction}
                onClick={() =>
                  onDecide({
                    kind: plan ? "pm_interval" : "one_off_wo",
                    pmPlanId: plan?.pm_plan_id ?? null,
                    intervalHours: proposed,
                    plannedCost: cost.planned_cost,
                    failureCost: cost.failure_cost,
                    title: t("rams.memo.decisionTitle", { interval: formatNumber(proposed), unit }),
                  })
                }
              >
                <ClipboardCheck className="h-4 w-4" aria-hidden />
                {t("rams.memo.record")}
              </Button>
            </div>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}

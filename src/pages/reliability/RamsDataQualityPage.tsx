/**
 * Reliability module — data quality: what keeps failure records out of the analyses, each with
 * the record to open or the action that fixes it (replaces the former "foundation" screen).
 */
import { ArrowLeft, ClipboardCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { KpiTile } from "@/components/metrics/KpiTile";
import {
  RAMS_FILTER_LABEL,
  RAMS_SELECT_CLASS,
  RAMS_TABLE_FRAME,
  RamsCard,
} from "@/components/reliability/study/rams-ui";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type DataQualityView,
  getReliabilityDataQuality,
} from "@/services/reliability-model-service";
import {
  dismissRamDataQualityIssue,
  refreshReliabilityKpiSnapshot,
  upsertRuntimeExposureLog,
} from "@/services/reliability-service";
import { P } from "@shared/rbac/permissions.generated";

const PERIODS = [12, 24, 36] as const;

export function RamsDataQualityPage() {
  const { t } = useTranslation("reliability");
  const { formatDate, formatNumber, formatDecimal } = useFormatters();
  const navigate = useNavigate();
  const [months, setMonths] = useState<number>(24);
  const [view, setView] = useState<DataQualityView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [hoursTarget, setHoursTarget] = useState<{ id: number; code: string } | null>(null);
  const [day, setDay] = useState("");
  const [hours, setHours] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setView(await getReliabilityDataQuality(months));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [months]);

  useEffect(() => {
    void load();
  }, [load]);

  /** Runs one correction and reloads the worklist; false when it failed (IPC toast shown). */
  const act = async (key: string, fn: () => Promise<unknown>): Promise<boolean> => {
    setBusy(key);
    try {
      await fn();
      await load();
      return true;
    } catch {
      return false;
    } finally {
      setBusy(null);
    }
  };

  const hoursN = Number(hours.replace(",", "."));
  const share = view && view.records > 0 ? view.records_complete / view.records : null;
  return (
    <ModulePageShell
      headerLeft={
        <div className="flex min-w-0 items-center gap-3">
          <Button variant="outline" className="h-11" onClick={() => navigate("/reliability")}>
            <ArrowLeft className="h-4 w-4" aria-hidden />
            {t("rams.study.back")}
          </Button>
          <h1 className={mfLayout.moduleTitle}>{t("rams.quality.title")}</h1>
        </div>
      }
      actions={<RefreshButton onRefresh={() => void load()} loading={loading} />}
      filterBar={
        <div className={mfLayout.moduleFilterBar}>
          <label className="flex items-center gap-2">
            <span className={RAMS_FILTER_LABEL}>{t("rams.fleet.period")}</span>
            <select
              className={RAMS_SELECT_CLASS}
              value={months}
              onChange={(e) => setMonths(Number(e.target.value))}
            >
              {PERIODS.map((m) => (
                <option key={m} value={m}>
                  {t("rams.fleet.periodMonths", { count: m })}
                </option>
              ))}
            </select>
          </label>
        </div>
      }
    >
      {error != null && view == null ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : view == null ? (
        <LoadingState />
      ) : (
        <div id="ram-data-quality" className="grid gap-4">
          <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
            <KpiTile
              label={t("rams.quality.complete")}
              value={share == null ? "—" : formatDecimal(share * 100, 0)}
              {...(share == null ? {} : { unit: "%" })}
              hint={t("rams.quality.completeHint", {
                complete: view.records_complete,
                total: view.records,
              })}
            />
            <KpiTile
              label={t("rams.quality.codingGaps")}
              value={formatNumber(view.coding_gaps.length)}
            />
            <KpiTile
              label={t("rams.quality.noHours")}
              value={formatNumber(view.no_operating_hours.length)}
            />
            <KpiTile
              label={t("rams.quality.review")}
              value={formatNumber(view.review_issues.length)}
            />
          </div>

          <RamsCard
            title={t("rams.quality.codingTitle")}
            subtitle={t("rams.quality.codingHint")}
            headingId="dq-coding"
          >
            {view.coding_gaps.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.quality.noCodingGap")}</p>
            ) : (
              <div className={RAMS_TABLE_FRAME}>
                <div className="max-h-[420px] overflow-auto">
                  <table className="w-full text-sm" aria-labelledby="dq-coding">
                    <thead>
                      <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                        <th className="px-3 py-2 text-start">{t("rams.data.colSource")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.data.colAsset")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.quality.closedAt")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.quality.gap")}</th>
                        <th className="px-3 py-2" />
                      </tr>
                    </thead>
                    <tbody>
                      {view.coding_gaps.map((g) => (
                        <tr
                          key={`${g.work_order_id}-${g.gap}`}
                          className="h-11 border-t border-surface-border"
                        >
                          <td className="px-3">
                            <LinkedEntityBadge
                              entity="work_order"
                              code={g.work_order_code}
                              entityId={g.work_order_id}
                            />
                          </td>
                          <td className="px-3 font-mono text-xs">{g.asset_code ?? "—"}</td>
                          <td className="px-3">{g.closed_at ? formatDate(g.closed_at) : "—"}</td>
                          <td className="px-3">
                            <StatusChip tone="warning" label={t(`rams.quality.gapKind.${g.gap}`)} />
                          </td>
                          <td className="px-3 text-end">
                            <Button
                              variant="outline"
                              className="h-10"
                              onClick={() => navigate(`/work-orders?openWo=${g.work_order_id}`)}
                            >
                              {t("rams.checks.openWo")}
                            </Button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </RamsCard>

          <RamsCard
            title={t("rams.quality.hoursTitle")}
            subtitle={t("rams.quality.hoursHint")}
            headingId="dq-hours"
          >
            {view.no_operating_hours.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.quality.noHoursGap")}</p>
            ) : (
              <div className={RAMS_TABLE_FRAME}>
                <table className="w-full text-sm" aria-labelledby="dq-hours">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.fleet.colAsset")}</th>
                      <th className="px-3 py-2 text-end">{t("rams.fleet.colFailures")}</th>
                      <th className="px-3 py-2" />
                    </tr>
                  </thead>
                  <tbody>
                    {view.no_operating_hours.map((e) => (
                      <tr key={e.equipment_id} className="h-11 border-t border-surface-border">
                        <td className="px-3">
                          <span className="font-mono text-xs font-semibold">{e.asset_code}</span>{" "}
                          {e.asset_name}
                        </td>
                        <td className="px-3 text-end font-mono">{formatNumber(e.failures)}</td>
                        <td className="px-3 text-end">
                          <PermissionGate permission={P.RAM_MANAGE}>
                            <Button
                              variant="outline"
                              className="h-10"
                              onClick={() => {
                                setDay(new Date().toISOString().slice(0, 10));
                                setHours("");
                                setHoursTarget({ id: e.equipment_id, code: e.asset_code });
                              }}
                            >
                              {t("rams.quality.addHours")}
                            </Button>
                          </PermissionGate>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </RamsCard>

          <RamsCard
            title={t("rams.quality.reviewTitle")}
            subtitle={t("rams.quality.reviewHint")}
            headingId="dq-review"
          >
            {view.review_issues.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.quality.noReview")}</p>
            ) : (
              <ul className="grid gap-2">
                {view.review_issues.map((i) => {
                  const key = `${i.issue_code}:${i.equipment_id}`;
                  return (
                    <li
                      key={key}
                      className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-surface-border px-3 py-2"
                    >
                      <div className="min-w-0">
                        <p className="text-sm font-medium">
                          <span className="font-mono text-xs">{i.asset_code}</span> {i.asset_name}
                        </p>
                        <p className="text-xs text-text-muted">
                          {t(`rams.quality.issue.${i.issue_code}`)}
                        </p>
                      </div>
                      <div className={cn("flex flex-wrap gap-2")}>
                        {i.issue_code === "MISSING_INSPECTION_COVERAGE" ? (
                          <Button
                            variant="outline"
                            className="h-10"
                            onClick={() => navigate("/inspections")}
                          >
                            <ClipboardCheck className="h-4 w-4" aria-hidden />
                            {t("rams.quality.openInspections")}
                          </Button>
                        ) : null}
                        <PermissionGate permission={P.RAM_MANAGE}>
                          {i.issue_code === "LOW_SAMPLE" ? (
                            <Button
                              variant="outline"
                              className="h-10"
                              disabled={busy != null}
                              onClick={() =>
                                void act(key, () => {
                                  const end = new Date();
                                  const start = new Date(end);
                                  start.setMonth(start.getMonth() - months);
                                  return refreshReliabilityKpiSnapshot({
                                    equipment_id: i.equipment_id,
                                    period_start: start.toISOString(),
                                    period_end: end.toISOString(),
                                    min_sample_n: null,
                                    repeat_lookback_days: null,
                                  });
                                })
                              }
                            >
                              {t("rams.quality.refreshKpi")}
                            </Button>
                          ) : null}
                          <Button
                            variant="ghost"
                            className="h-10"
                            disabled={busy != null}
                            onClick={() =>
                              void act(key, () =>
                                dismissRamDataQualityIssue({
                                  equipment_id: i.equipment_id,
                                  issue_code: i.issue_code,
                                }),
                              )
                            }
                          >
                            {t("rams.quality.dismiss")}
                          </Button>
                        </PermissionGate>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </RamsCard>
        </div>
      )}
      <Dialog open={hoursTarget != null} onOpenChange={(o) => !o && setHoursTarget(null)}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>
              {t("rams.quality.hoursDialogTitle", { code: hoursTarget?.code ?? "" })}
            </DialogTitle>
            <DialogDescription>{t("rams.quality.hoursDialogHint")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <label className="grid gap-1 text-xs font-medium text-text-secondary">
              {t("rams.quality.hoursDate")}
              <Input
                type="date"
                className="h-10"
                value={day}
                onChange={(e) => setDay(e.target.value)}
              />
            </label>
            <label className="grid gap-1 text-xs font-medium text-text-secondary">
              {t("rams.quality.hoursValue")}
              <Input
                inputMode="decimal"
                className="h-10"
                value={hours}
                onChange={(e) => setHours(e.target.value)}
              />
            </label>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setHoursTarget(null)}>
              {t("rams.common.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={!day || !(Number.isFinite(hoursN) && hoursN > 0) || busy != null}
              onClick={() => {
                const target = hoursTarget;
                if (!target) return;
                void act(`hours:${target.id}`, () =>
                  upsertRuntimeExposureLog({
                    id: null,
                    expected_row_version: null,
                    equipment_id: target.id,
                    exposure_type: "hours",
                    value: hoursN,
                    recorded_at: `${day}T23:59:59Z`,
                    source_type: "manual",
                  }),
                ).then((ok) => {
                  if (ok) setHoursTarget(null);
                });
              }}
            >
              {t("rams.quality.saveHours")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </ModulePageShell>
  );
}

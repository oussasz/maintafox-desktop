/**
 * Reliability (RAMS) module — fleet view ("Parc"): KPIs of the filtered fleet over a period and
 * the equipment ranked by downtime; select equipment of one kind and open a study.
 */
import { Activity, Archive, ArrowRight, FolderOpen, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { KpiTile } from "@/components/metrics/KpiTile";
import {
  RAMS_FILTER_LABEL,
  RAMS_PERIOD_MONTHS,
  RAMS_SELECT_CLASS,
  RAMS_TABLE_FRAME,
  RamsCallout,
  RamsCard,
  type RamsPeriodMonths,
  periodFromMonths,
} from "@/components/reliability/study/rams-ui";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState, TruncationNotice } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_DATA_QUALITY_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { getAssetById } from "@/services/asset-service";
import {
  type FleetAssetRow,
  type FleetView,
  type ReliabilityStudy,
  createReliabilityStudy,
  getReliabilityFleetView,
  listReliabilityStudies,
  updateReliabilityStudy,
} from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

const PAGE = 50;
/** Data-quality worklist of the reliability module (records without mode / cause, exposure gaps). */
const DATA_QUALITY_WORKLIST = "/reliability/data-quality";

export function RamsFleetPage() {
  const { t } = useTranslation("reliability");
  const { formatNumber, formatDecimal, formatPercent, formatDate } = useFormatters();
  const navigate = useNavigate();
  const { can } = usePermissions();
  const [months, setMonths] = useState<RamsPeriodMonths>(24);
  const [orgNodeId, setOrgNodeId] = useState<number | null>(null);
  const [classId, setClassId] = useState<number | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [view, setView] = useState<FleetView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<number[]>([]);
  // An equipment handed over in the address (/reliability?equipment=<id>, e.g. from the asset
  // record): selected for a study, named, or the failure shown.
  const [searchParams] = useSearchParams();
  const handedId = Number(searchParams.get("equipment") ?? "");
  const [handed, setHanded] = useState<{ id: number; code: string } | null>(null);
  const [handedError, setHandedError] = useState<unknown>(null);
  useEffect(() => {
    if (!Number.isInteger(handedId) || handedId <= 0) return;
    let cancelled = false;
    setHandedError(null);
    getAssetById(handedId)
      .then((a) => {
        if (cancelled) return;
        setHanded({ id: a.id, code: a.asset_code });
        setSelected((prev) => (prev.includes(a.id) ? prev : [...prev, a.id]));
      })
      .catch((e: unknown) => {
        if (!cancelled) setHandedError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [handedId]);
  const [studies, setStudies] = useState<ReliabilityStudy[]>([]);
  const [creating, setCreating] = useState(false);
  const [archiveTarget, setArchiveTarget] = useState<ReliabilityStudy | null>(null);

  const period = useMemo(() => periodFromMonths(months), [months]);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [v, s] = await Promise.all([
        getReliabilityFleetView({
          period_start: period.start,
          period_end: period.end,
          org_node_id: orgNodeId,
          equipment_class_id: classId,
          limit: PAGE,
          offset: 0,
        }),
        listReliabilityStudies(false),
      ]);
      setView(v);
      setStudies(s);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [period, orgNodeId, classId]);

  /** Next page of the ranking (the backend serves it by offset; the KPIs already cover all). */
  const loadMore = useCallback(async () => {
    if (!view) return;
    setLoadingMore(true);
    try {
      const next = await getReliabilityFleetView({
        period_start: period.start,
        period_end: period.end,
        org_node_id: orgNodeId,
        equipment_class_id: classId,
        limit: PAGE,
        offset: view.rows.length,
      });
      setView((v) =>
        v ? { ...v, rows: [...v.rows, ...next.rows], total_rows: next.total_rows } : next,
      );
    } catch {
      // The global IPC toast reports the failure; the rows already shown stay.
    } finally {
      setLoadingMore(false);
    }
  }, [view, period, orgNodeId, classId]);

  useEffect(() => {
    void load();
  }, [load]);

  const rows = view?.rows ?? [];
  const selectedRows = rows.filter((r) => selected.includes(r.equipment_id));
  const classes = new Set(selectedRows.map((r) => r.class_id ?? -1));
  const mixedClasses = classes.size > 1;
  const periodLabel = t("rams.fleet.periodMonths", { count: months });

  const toggle = (id: number) =>
    setSelected((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  const startStudy = async () => {
    if (selected.length === 0) return;
    setCreating(true);
    try {
      const study = await createReliabilityStudy({
        equipment_ids: selected,
        period_start: period.start,
        period_end: period.end,
      });
      navigate(`/reliability/studies/${study.id}`);
    } catch {
      // The global IPC toast reports the failure.
    } finally {
      setCreating(false);
    }
  };

  const k = view?.kpis;
  const basisHint =
    k == null
      ? ""
      : k.calendar_basis_count === 0
        ? t("rams.fleet.basisOperating")
        : k.operating_basis_count === 0
          ? t("rams.fleet.basisCalendar")
          : t("rams.fleet.basisMixed", {
              operating: k.operating_basis_count,
              calendar: k.calendar_basis_count,
            });
  const hours = (v: number | null | undefined) => (v == null ? "—" : formatNumber(Math.round(v)));

  return (
    <ModulePageShell
      icon={Activity}
      title={t("rams.title")}
      count={view?.total_rows ?? null}
      actions={
        <>
          <PermissionGate permission={P.RAM_ANALYZE}>
            <Button
              className={mfLayout.primaryAction}
              disabled={selected.length === 0 || creating}
              onClick={() => void startStudy()}
              title={selected.length === 0 ? t("rams.fleet.selectFirst") : undefined}
            >
              <ArrowRight className="h-4 w-4" aria-hidden />
              {selected.length === 0
                ? t("rams.fleet.studySelection")
                : t("rams.fleet.studyN", { count: selected.length })}
            </Button>
          </PermissionGate>
          <Button
            variant="outline"
            className="h-11"
            onClick={() => navigate(DATA_QUALITY_WORKLIST)}
          >
            <ShieldCheck className="h-4 w-4" aria-hidden />
            {t("rams.quality.title")}
          </Button>
          <RefreshButton onRefresh={() => void load()} loading={loading} />
        </>
      }
      filterBar={
        <div className={mfLayout.moduleFilterBar}>
          <label className="flex items-center gap-2">
            <span className={RAMS_FILTER_LABEL}>{t("rams.fleet.period")}</span>
            <select
              className={RAMS_SELECT_CLASS}
              value={months}
              onChange={(e) => {
                setMonths(Number(e.target.value) as RamsPeriodMonths);
              }}
            >
              {RAMS_PERIOD_MONTHS.map((m) => (
                <option key={m} value={m}>
                  {t("rams.fleet.periodMonths", { count: m })}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-2">
            <span className={RAMS_FILTER_LABEL}>{t("rams.fleet.site")}</span>
            <select
              className={RAMS_SELECT_CLASS}
              value={orgNodeId ?? ""}
              onChange={(e) => {
                setOrgNodeId(e.target.value ? Number(e.target.value) : null);
              }}
            >
              <option value="">{t("rams.fleet.allSites")}</option>
              {(view?.org_nodes ?? []).map((n) => (
                <option key={n.id} value={n.id}>
                  {`${"  ".repeat(Math.max(0, n.depth))}${n.label}`}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-2">
            <span className={RAMS_FILTER_LABEL}>{t("rams.fleet.class")}</span>
            <select
              className={RAMS_SELECT_CLASS}
              value={classId ?? ""}
              onChange={(e) => {
                setClassId(e.target.value ? Number(e.target.value) : null);
              }}
            >
              <option value="">{t("rams.fleet.allClasses")}</option>
              {(view?.equipment_classes ?? []).map((c) => (
                <option key={c.id} value={c.id}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      }
    >
      {handedError != null ? (
        <ErrorState error={handedError} title={t("rams.fleet.handedError")} className="mb-4 py-4" />
      ) : handed != null ? (
        <p className="mb-4 text-sm text-text-secondary" role="status">
          {t("rams.fleet.handedSelected", { code: handed.code })}
        </p>
      ) : null}
      {error != null && view == null ? (
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      ) : view == null ? (
        <LoadingState />
      ) : (
        <div className="grid gap-4">
          <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
            <KpiTile
              label={t("rams.kpi.availability")}
              value={
                k?.availability_operational == null
                  ? "—"
                  : formatDecimal(k.availability_operational * 100, 1)
              }
              unit="%"
              hint={t("rams.kpi.availabilityHint", { period: periodLabel })}
            />
            <KpiTile
              label={t("rams.kpi.mtbf")}
              value={hours(k?.mtbf_hours)}
              unit="h"
              hint={basisHint}
            />
            <KpiTile
              label={t("rams.kpi.mttr")}
              value={k?.mttr_hours == null ? "—" : formatDecimal(k.mttr_hours, 1)}
              unit="h"
              hint={t("rams.kpi.mttrHint")}
            />
            <KpiTile
              label={t("rams.kpi.failures")}
              value={formatNumber(k?.failure_count ?? 0)}
              hint={t("rams.kpi.failuresHint", {
                hours: formatNumber(Math.round(k?.downtime_hours ?? 0)),
              })}
            />
            <KpiTile
              label={t("rams.kpi.dataQuality")}
              value={k?.data_quality == null ? "—" : formatDecimal(k.data_quality * 100, 0)}
              {...(k?.data_quality == null ? {} : { unit: "%" })}
              {...(k?.data_quality != null && k.data_quality < 0.8
                ? { tone: "warning" as const }
                : {})}
              hint={t("rams.kpi.dataQualityHint", {
                mode: k?.records_missing_mode ?? 0,
                cause: k?.records_missing_cause ?? 0,
              })}
              onOpen={() => navigate(DATA_QUALITY_WORKLIST)}
            />
          </div>

          <RamsCard
            title={t("rams.fleet.badActorsTitle")}
            subtitle={t("rams.fleet.badActorsHint")}
            headingId="rams-bad-actors"
          >
            {mixedClasses ? (
              <div className="mb-3">
                <RamsCallout tone="warning" title={t("rams.fleet.mixedClassesTitle")}>
                  {t("rams.fleet.mixedClassesBody")}
                </RamsCallout>
              </div>
            ) : null}
            {rows.length === 0 ? (
              <EmptyState
                title={t("rams.fleet.emptyTitle")}
                description={t("rams.fleet.emptyBody")}
              />
            ) : (
              <div className={RAMS_TABLE_FRAME}>
                <div className="overflow-x-auto">
                  <table className="w-full text-sm" aria-labelledby="rams-bad-actors">
                    <thead>
                      <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                        <th className="w-11 px-3 py-2">
                          <span className="sr-only">{t("rams.fleet.colSelect")}</span>
                        </th>
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colAsset")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colClass")}</th>
                        <th className="px-3 py-2 text-end">{t("rams.fleet.colFailures")}</th>
                        <th className="px-3 py-2 text-end">{t("rams.fleet.colDowntime")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colShare")}</th>
                        <th className="px-3 py-2 text-end">{t("rams.fleet.colMtbf")}</th>
                        <th className="px-3 py-2 text-end">{t("rams.fleet.colMttr")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colLastStudy")}</th>
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colData")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((r) => (
                        <FleetRow
                          key={r.equipment_id}
                          row={r}
                          picked={selected.includes(r.equipment_id)}
                          canSelect={can(P.RAM_ANALYZE)}
                          onToggle={() => toggle(r.equipment_id)}
                          onOpenStudy={(id) => navigate(`/reliability/studies/${id}`)}
                          formatNumber={formatNumber}
                          formatDecimal={formatDecimal}
                          formatPercent={formatPercent}
                          formatDate={formatDate}
                        />
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
            {rows.length < view.total_rows ? (
              <TruncationNotice
                className="mt-3"
                shown={rows.length}
                total={view.total_rows}
                onLoadMore={() => void loadMore()}
                loadingMore={loadingMore}
              />
            ) : null}
          </RamsCard>

          <RamsCard
            title={t("rams.studies.title")}
            subtitle={t("rams.studies.hint")}
            headingId="rams-studies"
          >
            {studies.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.studies.empty")}</p>
            ) : (
              <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
                {studies.map((s) => (
                  <li
                    key={s.id}
                    className="flex flex-wrap items-center justify-between gap-2 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-text-primary">{s.title}</p>
                      <p className="text-xs text-text-muted">
                        {t("rams.studies.meta", {
                          assets: s.assets.map((a) => a.asset_code).join(", "),
                          from: formatDate(s.period_start),
                          to: formatDate(s.period_end),
                          updated: formatDate(s.updated_at),
                          decisions: s.decision_count,
                        })}
                      </p>
                    </div>
                    <div className="flex gap-2">
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => navigate(`/reliability/studies/${s.id}`)}
                      >
                        <FolderOpen className="h-4 w-4" aria-hidden />
                        {t("rams.studies.open")}
                      </Button>
                      <PermissionGate permission={P.RAM_ANALYZE}>
                        <Button
                          variant="ghost"
                          className="h-10"
                          onClick={() => setArchiveTarget(s)}
                          aria-label={t("rams.studies.archiveNamed", { title: s.title })}
                        >
                          <Archive className="h-4 w-4" aria-hidden />
                          {t("rams.studies.archive")}
                        </Button>
                      </PermissionGate>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </RamsCard>
        </div>
      )}
      <ConfirmDialog
        open={archiveTarget != null}
        onOpenChange={(o) => {
          if (!o) setArchiveTarget(null);
        }}
        description={t("rams.studies.archiveConfirm", { title: archiveTarget?.title ?? "" })}
        confirmLabel={t("rams.studies.archiveAction")}
        onConfirm={async () => {
          if (!archiveTarget) return;
          await updateReliabilityStudy({
            id: archiveTarget.id,
            expected_row_version: archiveTarget.row_version,
            status: "archived",
          });
          await load();
        }}
      />
    </ModulePageShell>
  );
}

function FleetRow({
  row,
  picked,
  canSelect,
  onToggle,
  onOpenStudy,
  formatNumber,
  formatDecimal,
  formatPercent,
  formatDate,
}: {
  row: FleetAssetRow;
  picked: boolean;
  canSelect: boolean;
  onToggle: () => void;
  onOpenStudy: (id: number) => void;
  formatNumber: (v: number) => string;
  formatDecimal: (v: number, dp?: number) => string;
  formatPercent: (v: number) => string;
  formatDate: (v: string) => string;
}) {
  const { t } = useTranslation("reliability");
  const label = t("rams.fleet.selectAsset", { code: row.asset_code });
  return (
    <tr
      tabIndex={canSelect ? 0 : undefined}
      aria-selected={canSelect ? picked : undefined}
      onClick={canSelect ? onToggle : undefined}
      onKeyDown={(e) => {
        if (canSelect && (e.key === " " || e.key === "Enter") && e.target === e.currentTarget) {
          e.preventDefault();
          onToggle();
        }
      }}
      className={cn(
        "h-11 border-t border-surface-border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
        canSelect && "cursor-pointer hover:bg-surface-2",
        picked && "bg-primary-bg/60",
      )}
    >
      <td className="px-3">
        {canSelect ? (
          <input
            type="checkbox"
            className="h-4 w-4 accent-primary"
            checked={picked}
            tabIndex={-1}
            aria-label={label}
            onChange={onToggle}
            onClick={(e) => e.stopPropagation()}
          />
        ) : null}
      </td>
      <td className="px-3">
        <span className="font-mono text-xs font-semibold text-text-primary">{row.asset_code}</span>{" "}
        <span className="text-text-secondary">{row.asset_name}</span>
      </td>
      <td className="px-3 text-text-secondary">{row.class_name ?? "—"}</td>
      <td className="px-3 text-end font-mono tabular-nums">{formatNumber(row.failures)}</td>
      <td className="px-3 text-end font-mono tabular-nums">
        {formatNumber(Math.round(row.downtime_hours))}
      </td>
      <td className="px-3">
        <div
          className="h-2 min-w-[90px] overflow-hidden rounded bg-surface-3"
          role="img"
          aria-label={t("rams.fleet.shareLabel", {
            share: formatPercent(row.downtime_share * 100),
          })}
        >
          <span
            className="block h-full bg-primary"
            style={{ width: `${Math.round(row.downtime_share * 100)}%` }}
          />
        </div>
      </td>
      <td className="px-3 text-end font-mono tabular-nums">
        {row.mtbf_hours == null ? "—" : formatNumber(Math.round(row.mtbf_hours))}
      </td>
      <td className="px-3 text-end font-mono tabular-nums">
        {row.mttr_hours == null ? "—" : formatDecimal(row.mttr_hours, 1)}
      </td>
      <td className="px-3">
        {row.last_study_id != null ? (
          <button
            type="button"
            className="min-h-9 text-start text-sm font-medium text-primary underline underline-offset-2"
            onClick={(e) => {
              e.stopPropagation();
              if (row.last_study_id != null) onOpenStudy(row.last_study_id);
            }}
          >
            {formatDate(row.last_study_updated_at ?? "")}
          </button>
        ) : (
          <span className="text-text-muted">{t("rams.fleet.noStudy")}</span>
        )}
      </td>
      <td className="px-3">
        <StatusChip
          tone={toneOf(RELIABILITY_DATA_QUALITY_TONE, row.data_quality)}
          label={t(`rams.dataQuality.${row.data_quality}`, { count: row.records_to_check })}
        />
      </td>
    </tr>
  );
}

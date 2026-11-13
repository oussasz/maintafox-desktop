/**
 * Reliability (RAMS) module — fleet view ("Parc"): KPIs of the filtered fleet over a period and
 * the equipment ranked by downtime; select equipment of one kind and open a study.
 */
import {
  Activity,
  Archive,
  ArrowDown,
  ArrowRight,
  ArrowUp,
  ArrowUpDown,
  ClipboardCheck,
  FolderOpen,
  Pencil,
  Search,
  ShieldCheck,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { KpiTile } from "@/components/metrics/KpiTile";
import { StudyNameDialog } from "@/components/reliability/study/StudyNameDialog";
import {
  RAMS_DEFAULT_MONTHS,
  RAMS_FILTER_LABEL,
  RAMS_PERIOD_MONTHS,
  RAMS_SELECT_CLASS,
  RAMS_TABLE_FRAME,
  RamsCallout,
  RamsCard,
  type RamsPeriodMonths,
  dataQualityHref,
  periodFromMonths,
} from "@/components/reliability/study/rams-ui";
import { useFleetKpiText } from "@/components/reliability/use-fleet-kpi-text";
import { ActionReason } from "@/components/ui/action-reason";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState, TruncationNotice } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { RELIABILITY_DATA_QUALITY_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { idParam, listLinks } from "@/lib/list-links";
import { cn } from "@/lib/utils";
import { getAssetById } from "@/services/asset-service";
import {
  type FleetAssetRow,
  type FleetSortKey,
  type FleetView,
  type ReliabilityStudy,
  createReliabilityStudy,
  getReliabilityFleetView,
  listReliabilityStudies,
  updateReliabilityStudy,
} from "@/services/reliability-study-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

const PAGE = 50;
/** Rows of the "10 most failing / 10 most costly" views. */
const TOP_N = 10;
/** Ranking columns the backend can order by. */
const SORT_KEYS: readonly FleetSortKey[] = ["failures", "downtime", "mtbf", "cost"];

/** Ids of a `1,2,3` address value (the equipment ticked for a study). */
function parseIds(raw: string | null): number[] {
  if (!raw) return [];
  return raw
    .split(",")
    .map((x) => Number(x))
    .filter((n) => Number.isInteger(n) && n > 0);
}
/** Step of a study an analysis method opens on (the step keys of the study page). */
type AnalysisStep = "life" | "fmeca" | "rcm";
/** The analysis methods offered on the home page, each with the study step it opens. */
const ANALYSIS_METHODS: readonly { key: string; step: AnalysisStep }[] = [
  { key: "weibull", step: "life" },
  { key: "fmeca", step: "fmeca" },
  { key: "rcm", step: "rcm" },
  { key: "ishikawa", step: "fmeca" },
];

export function RamsFleetPage() {
  const { t } = useTranslation("reliability");
  const fleetText = useFleetKpiText();
  const { formatNumber, formatDecimal, formatPercent, formatDate, formatCurrency, formatTime } =
    useFormatters();
  const navigate = useNavigate();
  const { can } = usePermissions();
  const [loadingMore, setLoadingMore] = useState(false);
  // Set when an analysis was asked for before any equipment was ticked.
  const [needSelection, setNeedSelection] = useState(false);
  const [view, setView] = useState<FleetView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  // An equipment handed over in the address (/reliability?equipment=<id>, e.g. from the asset
  // record): selected for a study, named, or the failure shown.
  const [searchParams, setSearchParams] = useSearchParams();
  // Period, organisation node and class live in the address (replace, so Retour is not polluted):
  // coming back from a study restores them.
  const monthsParam = Number(searchParams.get("months"));
  const months: RamsPeriodMonths = (RAMS_PERIOD_MONTHS as readonly number[]).includes(monthsParam)
    ? (monthsParam as RamsPeriodMonths)
    : RAMS_DEFAULT_MONTHS;
  // The ranking order, the 10-row views and the ticked equipment live in the address too:
  // opening an equipment or a study and coming back keeps them.
  const sortParam = searchParams.get("sort");
  const sortKey: FleetSortKey | null = (SORT_KEYS as readonly string[]).includes(sortParam ?? "")
    ? (sortParam as FleetSortKey)
    : null;
  const sortDesc = searchParams.get("dir")
    ? searchParams.get("dir") === "desc"
    : sortKey !== "mtbf";
  const topOnly = searchParams.get("top") === String(TOP_N);
  const selectedParam = searchParams.get("sel");
  const selected = useMemo(() => parseIds(selectedParam), [selectedParam]);
  const orgNodeId = idParam(searchParams.get("site"));
  const classId = idParam(searchParams.get("class"));
  const setQueryValue = (key: string, value: string | null) =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (value == null) next.delete(key);
        else next.set(key, value);
        return next;
      },
      { replace: true },
    );
  const updateSelected = (fn: (prev: number[]) => number[]) =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        const ids = fn(parseIds(prev.get("sel")));
        if (ids.length === 0) next.delete("sel");
        else next.set("sel", ids.join(","));
        return next;
      },
      { replace: true },
    );
  const updateSelectedRef = useRef(updateSelected);
  updateSelectedRef.current = updateSelected;
  const setRanking = (key: FleetSortKey | null, desc: boolean, top: boolean) =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (key == null) {
          next.delete("sort");
          next.delete("dir");
        } else {
          next.set("sort", key);
          next.set("dir", desc ? "desc" : "asc");
        }
        if (top) next.set("top", String(TOP_N));
        else next.delete("top");
        return next;
      },
      { replace: true },
    );
  /** Clicking a column orders by it; clicking it again reverses the order. */
  const sortBy = (key: FleetSortKey) =>
    setRanking(key, sortKey === key ? !sortDesc : key !== "mtbf", topOnly);
  const setMonths = (m: RamsPeriodMonths) =>
    setQueryValue("months", m === RAMS_DEFAULT_MONTHS ? null : String(m));
  const setOrgNodeId = (id: number | null) => setQueryValue("site", id == null ? null : String(id));
  const setClassId = (id: number | null) => setQueryValue("class", id == null ? null : String(id));
  /** Period, site and class back to their defaults in one click (UX L2-31). */
  const filtersActive = months !== RAMS_DEFAULT_MONTHS || orgNodeId != null || classId != null;
  const resetFilters = () =>
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("months");
        next.delete("site");
        next.delete("class");
        return next;
      },
      { replace: true },
    );
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
        updateSelectedRef.current((prev) => (prev.includes(a.id) ? prev : [...prev, a.id]));
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
  /** The naming dialog of a new study is open, with the step the analysis opens on. */
  const [createFor, setCreateFor] = useState<{ step: AnalysisStep | undefined } | null>(null);
  const [renameTarget, setRenameTarget] = useState<ReliabilityStudy | null>(null);
  const [archiveTarget, setArchiveTarget] = useState<ReliabilityStudy | null>(null);
  /** Search in the saved studies: name, equipment codes or failure mode. */
  const [query, setQuery] = useState("");

  const period = useMemo(() => periodFromMonths(months), [months]);
  /** When the figures on screen were last calculated (each load recalculates them live). */
  const [calculatedAt, setCalculatedAt] = useState<string | null>(null);
  // A failure figure opens the OTs behind it: the same failure records (period, class, site), so
  // the list holds what the figure counts (L2-21). Only for those who may read work orders.
  const canOpenOts = can(P.OT_VIEW);
  const failureOtsHref = (equipmentId?: number) =>
    listLinks.workOrders({
      ...(equipmentId != null ? { equipmentId } : {}),
      ramsFailures: { from: period.start, to: period.end, classId, orgNodeId },
    });

  const load = useCallback(async (): Promise<boolean> => {
    setLoading(true);
    setError(null);
    try {
      const [v, s] = await Promise.all([
        getReliabilityFleetView({
          period_start: period.start,
          period_end: period.end,
          org_node_id: orgNodeId,
          equipment_class_id: classId,
          limit: topOnly ? TOP_N : PAGE,
          offset: 0,
          sort_by: sortKey,
          sort_desc: sortKey == null ? null : sortDesc,
        }),
        listReliabilityStudies(false),
      ]);
      setView(v);
      setStudies(s);
      setCalculatedAt(new Date().toISOString());
      return true;
    } catch (e) {
      setError(e);
      return false;
    } finally {
      setLoading(false);
    }
  }, [period, orgNodeId, classId, sortKey, sortDesc, topOnly]);

  /** Header "Actualiser": recalculates and says so, with the time of the calculation. */
  const refresh = async () => {
    if (await load()) {
      pushAppToast({
        title: t("rams.fleet.refreshed", { time: formatTime(new Date().toISOString()) }),
        variant: "success",
      });
    }
  };

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
        sort_by: sortKey,
        sort_desc: sortKey == null ? null : sortDesc,
      });
      setView((v) =>
        v ? { ...v, rows: [...v.rows, ...next.rows], total_rows: next.total_rows } : next,
      );
    } catch {
      // The global IPC toast reports the failure; the rows already shown stay.
    } finally {
      setLoadingMore(false);
    }
  }, [view, period, orgNodeId, classId, sortKey, sortDesc]);

  useEffect(() => {
    void load();
  }, [load]);

  const rows = view?.rows ?? [];
  const selectedRows = rows.filter((r) => selected.includes(r.equipment_id));
  const classes = new Set(selectedRows.map((r) => r.class_id ?? -1));
  const mixedClasses = classes.size > 1;
  const mixedClassNames = mixedClasses
    ? [...new Set(selectedRows.map((r) => r.class_name ?? t("rams.studyName.noClass")))]
    : [];
  const needle = query.trim().toLocaleLowerCase();
  const shownStudies =
    needle === ""
      ? studies
      : studies.filter((s) =>
          [
            s.title,
            s.failure_mode_label ?? "",
            s.failure_mode_code ?? "",
            ...s.assets.map((a) => a.asset_code),
          ].some((v) => v.toLocaleLowerCase().includes(needle)),
        );
  const periodLabel = t("rams.fleet.periodMonths", { count: months });

  const toggle = (id: number) =>
    updateSelected((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  /** "Étudier": the study is named (or named automatically) before it is created (L2-13). */
  const startStudy = (step?: AnalysisStep) => {
    if (selected.length === 0) {
      setNeedSelection(true);
      return;
    }
    setCreateFor({ step });
  };

  /** Throws on a refusal (reported by the global IPC toast): the dialog stays open. */
  const createStudy = async (name: string, acceptMixed: boolean) => {
    setCreating(true);
    try {
      const study = await createReliabilityStudy({
        title: name || null,
        equipment_ids: selected,
        period_start: period.start,
        period_end: period.end,
        accept_mixed_classes: acceptMixed,
      });
      const step = createFor?.step;
      navigate(`/reliability/studies/${study.id}${step ? `?step=${step}` : ""}`);
    } finally {
      setCreating(false);
    }
  };

  const k = view?.kpis;
  const basisHint = fleetText.basis(k);
  const hours = (v: number | null | undefined) => (v == null ? "—" : formatNumber(Math.round(v)));
  const qualityHref = dataQualityHref(months, orgNodeId, classId);
  /** What the figures cover, named from the filters: the whole fleet, a site, a class or both. */
  const scopeLabel =
    [
      (view?.org_nodes ?? []).find((n) => n.id === orgNodeId)?.label,
      (view?.equipment_classes ?? []).find((c) => c.id === classId)?.label,
    ]
      .filter((x): x is string => x != null)
      .join(" · ") || t("rams.fleet.scopeFleet");
  // A class kept from another site that this scope does not have: drop it instead of showing an
  // empty fleet under a filter the list no longer offers (UX L2-31).
  const staleClass =
    classId != null && view != null && !view.equipment_classes.some((c) => c.id === classId);
  useEffect(() => {
    if (staleClass) setClassId(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- setClassId is rebuilt every render
  }, [staleClass]);
  const exclusions = fleetText.exclusions;
  /** Hint of a rate figure: its definition, then what was left out, or why it is not given. */
  const rateHint = (definition: string, excluded: string[]) =>
    k?.window_too_short ? fleetText.tooShort() : [definition, ...excluded].join(" · ");

  return (
    <ModulePageShell
      icon={Activity}
      title={t("rams.title")}
      count={view?.total_rows ?? null}
      countLabel={t("rams.fleet.countLabel", { count: view?.total_rows ?? 0 })}
      actions={
        <>
          <PermissionGate permission={P.RAM_ANALYZE}>
            <Button
              className={mfLayout.primaryAction}
              disabled={selected.length === 0 || creating}
              onClick={() => startStudy()}
              aria-describedby={selected.length === 0 ? "rams-study-reason" : undefined}
              title={selected.length === 0 ? t("rams.fleet.selectFirst") : undefined}
            >
              <ArrowRight className="h-4 w-4" aria-hidden />
              {selected.length === 0
                ? t("rams.fleet.studySelection")
                : t("rams.fleet.studyN", { count: selected.length })}
            </Button>
          </PermissionGate>
          <Button variant="outline" className="h-11" onClick={() => navigate(qualityHref)}>
            <ShieldCheck className="h-4 w-4" aria-hidden />
            {t("rams.quality.title")}
          </Button>
          <Button
            variant="outline"
            className="h-11"
            onClick={() => navigate("/inspections?tab=signals")}
            title={t("rams.fleet.signalsHint")}
          >
            <ClipboardCheck className="h-4 w-4" aria-hidden />
            {t("rams.fleet.signals")}
          </Button>
          <RefreshButton onRefresh={() => void refresh()} loading={loading} />
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
          {filtersActive ? (
            <Button variant="ghost" className="h-10" onClick={resetFilters}>
              {t("rams.fleet.resetFilters")}
            </Button>
          ) : null}
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
          <RamsCard
            title={t("rams.methods.title")}
            subtitle={t("rams.methods.hint")}
            headingId="rams-methods"
          >
            {needSelection && selected.length === 0 ? (
              <RamsCallout tone="warning" title={t("rams.methods.selectTitle")}>
                {t("rams.methods.selectBody")}
              </RamsCallout>
            ) : null}
            <ul className="grid grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-3">
              {ANALYSIS_METHODS.map((m) => (
                <li
                  key={m.key}
                  className="flex flex-col justify-between gap-2 rounded-md border border-surface-border p-3"
                >
                  <div>
                    <p className="text-sm font-medium text-text-primary">
                      {t(`rams.methods.${m.key}.title`)}
                    </p>
                    <p className="text-xs text-text-muted">{t(`rams.methods.${m.key}.line`)}</p>
                  </div>
                  <PermissionGate permission={P.RAM_ANALYZE}>
                    <Button
                      variant="outline"
                      className="h-10 justify-start"
                      disabled={creating}
                      onClick={() => startStudy(m.step)}
                    >
                      <ArrowRight className="h-4 w-4" aria-hidden />
                      {selected.length === 0
                        ? t("rams.methods.choose")
                        : t("rams.methods.start", { count: selected.length })}
                    </Button>
                  </PermissionGate>
                </li>
              ))}
              <li className="flex flex-col justify-between gap-2 rounded-md border border-surface-border p-3">
                <div>
                  <p className="text-sm font-medium text-text-primary">
                    {t("rams.methods.studies.title")}
                  </p>
                  <p className="text-xs text-text-muted">{t("rams.methods.studies.line")}</p>
                </div>
                <Button
                  variant="outline"
                  className="h-10 justify-start"
                  onClick={() =>
                    document
                      .getElementById("rams-studies")
                      ?.scrollIntoView({ behavior: "smooth", block: "start" })
                  }
                >
                  <FolderOpen className="h-4 w-4" aria-hidden />
                  {t("rams.methods.studies.open")}
                </Button>
              </li>
            </ul>
          </RamsCard>

          <div className="grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3">
            <KpiTile
              label={t("rams.kpi.availabilityOf", { scope: scopeLabel })}
              formula={t("rams.kpi.formula.availability")}
              value={
                k?.availability_operational == null
                  ? "—"
                  : formatDecimal(k.availability_operational * 100, 1)
              }
              unit="%"
              hint={rateHint(
                t("rams.kpi.availabilityHint", { period: periodLabel, basis: basisHint }),
                [],
              )}
            />
            <KpiTile
              label={t("rams.kpi.mtbf")}
              formula={t("rams.kpi.formula.mtbf")}
              value={hours(k?.mtbf_hours)}
              unit="h"
              hint={rateHint(
                t("rams.kpi.mtbfHint", { period: periodLabel, basis: basisHint }),
                exclusions([
                  ["outsideService", k?.mtbf_excluded_outside_service ?? 0],
                  ["missingDate", k?.mtbf_excluded_missing_date ?? 0],
                ]),
              )}
            />
            <KpiTile
              label={t("rams.kpi.mttr")}
              formula={t("rams.kpi.formula.mttr")}
              value={k?.mttr_hours == null ? "—" : formatDecimal(k.mttr_hours, 1)}
              unit="h"
              hint={rateHint(
                t("rams.kpi.mttrHint", { period: periodLabel }),
                exclusions([
                  ["failedAt", k?.restore_excluded_missing_failed_at ?? 0],
                  ["restoredAt", k?.restore_excluded_missing_restored_at ?? 0],
                  ["inconsistent", k?.restore_excluded_inconsistent ?? 0],
                ]),
              )}
            />
            <KpiTile
              label={t("rams.kpi.repair")}
              formula={t("rams.kpi.formula.repair")}
              value={
                k?.mean_active_repair_hours == null
                  ? "—"
                  : formatDecimal(k.mean_active_repair_hours, 1)
              }
              unit="h"
              hint={rateHint(
                t("rams.kpi.repairHint", { period: periodLabel }),
                exclusions([["repairHours", k?.repair_excluded_missing_hours ?? 0]]),
              )}
            />
            <KpiTile
              label={t("rams.kpi.failures")}
              formula={t("rams.kpi.formula.failures")}
              value={formatNumber(k?.failure_count ?? 0)}
              hint={t("rams.kpi.failuresHint", {
                period: periodLabel,
                hours: formatNumber(Math.max(0, Math.round(k?.downtime_hours ?? 0))),
              })}
              {...(canOpenOts && (k?.failure_count ?? 0) > 0
                ? { onOpen: () => navigate(failureOtsHref()) }
                : {})}
            />
            <KpiTile
              label={t("rams.kpi.dataQuality")}
              formula={t("rams.kpi.formula.dataQuality")}
              value={k?.data_quality == null ? "—" : formatDecimal(k.data_quality * 100, 0)}
              {...(k?.data_quality == null ? {} : { unit: "%" })}
              {...(k?.data_quality != null && k.data_quality < 0.8
                ? { tone: "warning" as const }
                : {})}
              hint={t("rams.kpi.dataQualityHint", {
                period: periodLabel,
                mode: k?.records_missing_mode ?? 0,
                cause: k?.records_missing_cause ?? 0,
                mechanism: k?.records_missing_mechanism ?? 0,
              })}
              onOpen={() => navigate(qualityHref)}
            />
          </div>

          <RamsCard
            title={t("rams.fleet.badActorsTitle")}
            subtitle={t("rams.fleet.badActorsHint")}
            headingId="rams-bad-actors"
            actions={
              <div
                className="flex flex-wrap gap-2"
                role="group"
                aria-label={t("rams.fleet.viewsLabel")}
              >
                {(
                  [
                    ["all", null, true, false],
                    ["topFailing", "failures", true, true],
                    ["topCostly", "cost", true, true],
                  ] as const
                ).map(([id, key, desc, top]) => {
                  const active = top === topOnly && key === sortKey;
                  return (
                    <Button
                      key={id}
                      variant={active ? "default" : "outline"}
                      className="h-9"
                      aria-pressed={active}
                      onClick={() => setRanking(key, desc, top)}
                    >
                      {t(`rams.fleet.view.${id}`, { count: TOP_N })}
                    </Button>
                  );
                })}
              </div>
            }
          >
            {calculatedAt ? (
              <p className="mb-2 text-xs text-text-muted" data-testid="rams-calculated-at">
                {t("rams.fleet.calculatedAt", {
                  date: formatDate(calculatedAt),
                  time: formatTime(calculatedAt),
                })}
              </p>
            ) : null}
            {mixedClasses ? (
              <div className="mb-3">
                <RamsCallout tone="warning" title={t("rams.fleet.mixedClassesTitle")}>
                  {t("rams.fleet.mixedClassesBody")}
                </RamsCallout>
              </div>
            ) : null}
            {rows.length > 0 && selected.length === 0 ? (
              <PermissionGate permission={P.RAM_ANALYZE}>
                <ActionReason
                  className="mb-2"
                  data-testid="rams-study-reason"
                  id="rams-study-reason"
                >
                  {t("rams.fleet.selectFirst")}
                </ActionReason>
              </PermissionGate>
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
                        <SortTh
                          label={t("rams.fleet.colFailures")}
                          sortKey="failures"
                          active={sortKey}
                          desc={sortDesc}
                          onSort={sortBy}
                        />
                        <SortTh
                          label={t("rams.fleet.colDowntime")}
                          sortKey="downtime"
                          active={sortKey}
                          desc={sortDesc}
                          onSort={sortBy}
                        />
                        <th className="px-3 py-2 text-start">{t("rams.fleet.colShare")}</th>
                        <SortTh
                          label={t("rams.fleet.colCost")}
                          sortKey="cost"
                          active={sortKey}
                          desc={sortDesc}
                          onSort={sortBy}
                        />
                        <SortTh
                          label={t("rams.fleet.colMtbf")}
                          sortKey="mtbf"
                          active={sortKey}
                          desc={sortDesc}
                          onSort={sortBy}
                        />
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
                          onOpenAsset={(id) => navigate(listLinks.asset(id))}
                          {...(canOpenOts
                            ? { onOpenFailures: () => navigate(failureOtsHref(r.equipment_id)) }
                            : {})}
                          formatNumber={formatNumber}
                          formatDecimal={formatDecimal}
                          formatPercent={formatPercent}
                          formatDate={formatDate}
                          formatCurrency={formatCurrency}
                        />
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
            {!topOnly && rows.length < view.total_rows ? (
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
            {studies.length > 0 ? (
              <label className="mb-3 flex max-w-sm items-center gap-2">
                <Search className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                <Input
                  className="h-10"
                  type="search"
                  value={query}
                  placeholder={t("rams.studies.searchPlaceholder")}
                  aria-label={t("rams.studies.search")}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </label>
            ) : null}
            {studies.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.studies.empty")}</p>
            ) : shownStudies.length === 0 ? (
              <p className="text-sm text-text-muted" role="status">
                {t("rams.studies.noMatch", { query: query.trim() })}
              </p>
            ) : (
              <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
                {shownStudies.map((s) => (
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
                          onClick={() => setRenameTarget(s)}
                          aria-label={t("rams.studies.renameNamed", { title: s.title })}
                        >
                          <Pencil className="h-4 w-4" aria-hidden />
                          {t("rams.studies.rename")}
                        </Button>
                      </PermissionGate>
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
      <StudyNameDialog
        open={createFor != null}
        onOpenChange={(o) => {
          if (!o) setCreateFor(null);
        }}
        mode="create"
        mixedClasses={mixedClassNames}
        onSubmit={createStudy}
      />
      <StudyNameDialog
        open={renameTarget != null}
        onOpenChange={(o) => {
          if (!o) setRenameTarget(null);
        }}
        mode="rename"
        initialName={renameTarget?.title ?? ""}
        onSubmit={async (name) => {
          if (!renameTarget) return;
          await updateReliabilityStudy({
            id: renameTarget.id,
            expected_row_version: renameTarget.row_version,
            title: name,
          });
          await load();
        }}
      />
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
  onOpenAsset,
  onOpenFailures,
  formatNumber,
  formatDecimal,
  formatPercent,
  formatDate,
  formatCurrency,
}: {
  row: FleetAssetRow;
  picked: boolean;
  canSelect: boolean;
  onToggle: () => void;
  onOpenStudy: (id: number) => void;
  onOpenAsset: (id: number) => void;
  /** Opens the OTs behind this row's failures (absent without the right to read OTs). */
  onOpenFailures?: () => void;
  formatNumber: (v: number) => string;
  formatDecimal: (v: number, dp?: number) => string;
  formatPercent: (v: number) => string;
  formatDate: (v: string) => string;
  formatCurrency: (v: number) => string;
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
        <span className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            className="min-h-9 font-mono text-xs font-semibold text-primary underline underline-offset-2"
            title={t("rams.fleet.openAsset", { code: row.asset_code })}
            onClick={(e) => {
              e.stopPropagation();
              onOpenAsset(row.equipment_id);
            }}
          >
            {row.asset_code}
          </button>
          <span className="text-text-secondary">{row.asset_name}</span>
        </span>
      </td>
      <td className="px-3 text-text-secondary">{row.class_name ?? "—"}</td>
      <td className="px-3 text-end font-mono tabular-nums">
        {onOpenFailures && row.failures > 0 ? (
          <button
            type="button"
            className="rounded px-1 text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            title={t("rams.fleet.openFailures", { code: row.asset_code })}
            aria-label={t("rams.fleet.openFailures", { code: row.asset_code })}
            onClick={(e) => {
              e.stopPropagation();
              onOpenFailures();
            }}
          >
            {formatNumber(row.failures)}
          </button>
        ) : (
          formatNumber(row.failures)
        )}
      </td>
      <td className="px-3 text-end font-mono tabular-nums">
        {formatNumber(Math.max(0, Math.round(row.downtime_hours)))}
      </td>
      <td className="px-3">
        <div className="flex items-center gap-2">
          <div
            className="h-2 min-w-[90px] flex-1 overflow-hidden rounded bg-surface-3"
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
          <span className="w-12 shrink-0 text-end font-mono text-xs tabular-nums text-text-secondary">
            {formatPercent(row.downtime_share * 100)}
          </span>
        </div>
      </td>
      <td className="px-3 text-end font-mono tabular-nums">
        {row.failure_cost == null ? "—" : formatCurrency(row.failure_cost)}
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

/** A sortable column header: the button orders the ranking, `aria-sort` tells the state. */
function SortTh({
  label,
  sortKey,
  active,
  desc,
  onSort,
}: {
  label: string;
  sortKey: FleetSortKey;
  active: FleetSortKey | null;
  desc: boolean;
  onSort: (key: FleetSortKey) => void;
}) {
  const on = active === sortKey;
  const Icon = !on ? ArrowUpDown : desc ? ArrowDown : ArrowUp;
  return (
    <th
      className="px-3 py-2 text-end"
      aria-sort={on ? (desc ? "descending" : "ascending") : "none"}
    >
      <button
        type="button"
        className="inline-flex min-h-9 items-center gap-1 uppercase tracking-wide hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => onSort(sortKey)}
      >
        {label}
        <Icon className="h-3 w-3" aria-hidden />
      </button>
    </th>
  );
}

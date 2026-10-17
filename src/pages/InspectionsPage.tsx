/**
 * Inspection rounds (PRD §6.25): module figures, rounds (field runner and supervisor review),
 * anomaly follow-up, recurring schedules, checklists / routes and reliability signals.
 * Layout: ModulePageShell (OT/DI pattern) with the SmartFilterBar of the active list.
 *
 * Deep link: `/inspections?openRound=<id>` opens a round in the runner.
 */
import { CalendarPlus, ClipboardCheck, Loader2, Plus, RefreshCcw } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { InspectionAnomaliesPanel } from "@/components/inspections/InspectionAnomaliesPanel";
import { InspectionKpiStrip } from "@/components/inspections/InspectionKpiStrip";
import {
  InspectionRoundsPanel,
  ScheduleRoundDialog,
} from "@/components/inspections/InspectionRoundsPanel";
import { InspectionRunner } from "@/components/inspections/InspectionRunner";
import {
  InspectionScheduleDialog,
  InspectionSchedulesPanel,
} from "@/components/inspections/InspectionSchedulesPanel";
import { InspectionSignalsPanel } from "@/components/inspections/InspectionSignalsPanel";
import {
  InspectionTemplatesPanel,
  TemplateBuilderDialog,
} from "@/components/inspections/InspectionTemplatesPanel";
import {
  ANOMALY_STATUSES,
  type InspectionKpiKey,
  ROUND_STATUSES,
  type RoundTiming,
  kpiDrillDown,
  roundFilterFor,
} from "@/components/inspections/inspection-rules";
import { FilterToggleButton, RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { generateInspectionRounds, getInspectionKpis } from "@/services/inspection-service";
import type {
  GenerateInspectionRoundsResult,
  InspectionKpis,
  InspectionScheduleSummary,
  InspectionTemplate,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type Tab = "rounds" | "anomalies" | "schedules" | "templates" | "signals";
const TABS: Tab[] = ["rounds", "anomalies", "schedules", "templates", "signals"];

const STATUS_GROUPS = ["todo", "toReview", "finished"] as const;

export function InspectionsPage() {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const [searchParams, setSearchParams] = useSearchParams();
  const [tab, setTab] = useState<Tab>("rounds");
  const [openRoundId, setOpenRoundId] = useState<number | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const bump = useCallback(() => setRefreshKey((k) => k + 1), []);

  // Figures
  const [periodDays, setPeriodDays] = useState(30);
  const [kpis, setKpis] = useState<InspectionKpis | null>(null);
  const [kpiError, setKpiError] = useState<unknown>(null);
  const [kpiLoading, setKpiLoading] = useState(true);

  // Lists
  const [showFilters, setShowFilters] = useState(true);
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [roundStatus, setRoundStatus] = useState<string | null>("todo");
  const [roundTiming, setRoundTiming] = useState<RoundTiming | null>(null);
  const [anomalyStatus, setAnomalyStatus] = useState<string | null>("open");
  const [count, setCount] = useState<number | null>(null);

  // Dialogs
  const [scheduleRoundOpen, setScheduleRoundOpen] = useState(false);
  const [templateTarget, setTemplateTarget] = useState<InspectionTemplate | "new" | null>(null);
  const [scheduleTarget, setScheduleTarget] = useState<InspectionScheduleSummary | "new" | null>(
    null,
  );
  const [generating, setGenerating] = useState(false);
  const [generated, setGenerated] = useState<GenerateInspectionRoundsResult | null>(null);

  useLayoutEffect(() => {
    const raw = searchParams.get("openRound");
    if (!raw) return;
    const id = Number.parseInt(raw, 10);
    if (!Number.isNaN(id)) setOpenRoundId(id);
    setSearchParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        p.delete("openRound");
        return p;
      },
      { replace: true },
    );
  }, [searchParams, setSearchParams]);

  const loadKpis = useCallback(async () => {
    setKpiLoading(true);
    try {
      setKpis(await getInspectionKpis({ period_days: periodDays }));
      setKpiError(null);
    } catch (err) {
      setKpiError(err);
    } finally {
      setKpiLoading(false);
    }
  }, [periodDays]);

  useEffect(() => {
    void loadKpis();
  }, [loadKpis, refreshKey]);

  const period = kpis ? { from: kpis.period_start, to: kpis.period_end } : null;
  const roundFilter = useMemo(
    () => roundFilterFor({ status: roundStatus, timing: roundTiming, period, search }),
    // `period` is derived from `kpis`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [roundStatus, roundTiming, kpis, search],
  );

  const openKpi = (key: InspectionKpiKey) => {
    const target = kpiDrillDown(key);
    setOpenRoundId(null);
    setShowFilters(true);
    if (target.tab === "anomalies") {
      setAnomalyStatus(target.anomalyStatus);
    } else {
      setRoundStatus(target.status);
      setRoundTiming(target.timing);
      setSearch("");
      setSearchInput("");
    }
    setTab(target.tab);
  };

  const generate = async () => {
    setGenerating(true);
    setGenerated(null);
    try {
      setGenerated(await generateInspectionRounds({}));
      bump();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setGenerating(false);
    }
  };

  const statusOptions = useMemo(
    () => [
      ...STATUS_GROUPS.map((g) => ({ value: g, label: t(`filters.${g}`) })),
      ...ROUND_STATUSES.map((s) => ({ value: s, label: t(`status.${s}`) })),
    ],
    [t],
  );

  const filterDefs = useMemo<SmartFilterDef[]>(() => {
    if (tab === "anomalies") {
      return [
        {
          id: "anomaly-status",
          kind: "select",
          label: t("anomalies.filterStatus"),
          options: ANOMALY_STATUSES.map((s) => ({ value: s, label: t(`anomalyStatus.${s}`) })),
          value: anomalyStatus,
          onChange: setAnomalyStatus,
        },
      ];
    }
    return [
      {
        id: "round-status",
        kind: "select",
        label: t("filters.status"),
        options: statusOptions,
        value: roundStatus,
        onChange: setRoundStatus,
      },
      {
        id: "round-timing",
        kind: "select",
        label: t("filters.timing"),
        options: [
          { value: "overdue", label: t("filters.overdue") },
          { value: "period", label: t("filters.period", { count: periodDays }) },
        ],
        value: roundTiming,
        onChange: (v) => setRoundTiming(v === "overdue" || v === "period" ? v : null),
      },
    ];
  }, [anomalyStatus, periodDays, roundStatus, roundTiming, statusOptions, t, tab]);

  const listTab = tab === "rounds" || tab === "anomalies";
  const activeFilterCount =
    tab === "anomalies"
      ? anomalyStatus
        ? 1
        : 0
      : [roundStatus, roundTiming, search || null].filter(Boolean).length;

  const resetFilters = () => {
    if (tab === "anomalies") {
      setAnomalyStatus(null);
      return;
    }
    setRoundStatus(null);
    setRoundTiming(null);
    setSearch("");
    setSearchInput("");
  };

  const primaryAction = (() => {
    if (openRoundId != null) return null;
    if (tab === "rounds")
      return (
        <PermissionGate permission={P.INS_ADMIN}>
          <Button className={mfLayout.primaryAction} onClick={() => setScheduleRoundOpen(true)}>
            <CalendarPlus className="h-4 w-4" aria-hidden />
            {t("rounds.schedule")}
          </Button>
        </PermissionGate>
      );
    if (tab === "schedules")
      return (
        <PermissionGate permission={P.INS_ADMIN}>
          <Button className={mfLayout.primaryAction} onClick={() => setScheduleTarget("new")}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("schedules.new")}
          </Button>
          <Button
            variant="outline"
            className="h-11 gap-1.5"
            disabled={generating}
            onClick={() => void generate()}
          >
            {generating ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <RefreshCcw className="h-4 w-4" aria-hidden />
            )}
            {t("schedules.generate")}
          </Button>
        </PermissionGate>
      );
    if (tab === "templates")
      return (
        <PermissionGate permission={P.INS_ADMIN}>
          <Button className={mfLayout.primaryAction} onClick={() => setTemplateTarget("new")}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("templates.new")}
          </Button>
        </PermissionGate>
      );
    return null;
  })();

  return (
    <ModulePageShell
      icon={ClipboardCheck}
      title={t("page.title")}
      description={t("page.description")}
      count={openRoundId == null ? count : null}
      bodyClassName="space-y-4 p-4"
      actions={
        <>
          {primaryAction}
          {listTab && openRoundId == null ? (
            <FilterToggleButton
              open={showFilters}
              onToggle={() => setShowFilters((v) => !v)}
              activeCount={activeFilterCount}
            />
          ) : null}
          <RefreshButton onRefresh={bump} loading={kpiLoading} />
        </>
      }
      filterBar={
        listTab && showFilters && openRoundId == null ? (
          <SmartFilterBar
            searchPlaceholder={t("filters.search")}
            searchValue={searchInput}
            onSearchInputChange={setSearchInput}
            onSearchChange={setSearch}
            showSearch={tab === "rounds"}
            filters={filterDefs}
            resultCount={count}
            onReset={resetFilters}
          />
        ) : undefined
      }
    >
      {openRoundId != null ? (
        <InspectionRunner
          roundId={openRoundId}
          onClose={() => {
            setOpenRoundId(null);
            bump();
          }}
          onChanged={bump}
        />
      ) : (
        <>
          <InspectionKpiStrip
            kpis={kpis}
            error={kpiError}
            loading={kpiLoading}
            periodDays={periodDays}
            onPeriodChange={setPeriodDays}
            onOpen={openKpi}
            onRetry={() => void loadKpis()}
          />

          {generated ? (
            <div
              role="status"
              className="space-y-1 rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-sm text-tone-info-fg"
              data-testid="generation-result"
            >
              <p>
                {t("schedules.generated", {
                  generated: generated.generated,
                  skipped: generated.skipped,
                  missed: generated.missed,
                })}
              </p>
              {generated.issues.map((issue) => (
                <p key={issue.schedule_code} className="text-tone-warning-fg">
                  {t("schedules.issue", { code: issue.schedule_code })}{" "}
                  {t(`errors:gateError.${issue.message.split(":")[0] ?? ""}`, {
                    defaultValue: t("errors:generic.unexpected"),
                  })}
                </p>
              ))}
            </div>
          ) : null}

          <Tabs
            value={tab}
            onValueChange={(v) => {
              setTab(v as Tab);
              setCount(null);
            }}
            className="space-y-4"
          >
            <TabsList className="h-12 flex-wrap">
              {TABS.map((key) => (
                <TabsTrigger key={key} value={key} className="min-h-11 px-4 text-sm">
                  {t(`tabs.${key}`)}
                </TabsTrigger>
              ))}
            </TabsList>
            <TabsContent value="rounds">
              <InspectionRoundsPanel
                filter={roundFilter}
                onOpenRound={setOpenRoundId}
                refreshKey={refreshKey}
                onCountChange={setCount}
              />
            </TabsContent>
            <TabsContent value="anomalies">
              <InspectionAnomaliesPanel
                status={anomalyStatus}
                refreshKey={refreshKey}
                onChanged={bump}
                onOpenRound={setOpenRoundId}
                onCountChange={setCount}
              />
            </TabsContent>
            <TabsContent value="schedules">
              <InspectionSchedulesPanel
                refreshKey={refreshKey}
                onEdit={setScheduleTarget}
                onCountChange={setCount}
              />
            </TabsContent>
            <TabsContent value="templates">
              <InspectionTemplatesPanel
                refreshKey={refreshKey}
                onEdit={setTemplateTarget}
                onCountChange={setCount}
              />
            </TabsContent>
            <TabsContent value="signals">
              <InspectionSignalsPanel refreshKey={refreshKey} />
            </TabsContent>
          </Tabs>
        </>
      )}

      {can(P.INS_ADMIN) ? (
        <>
          <ScheduleRoundDialog
            open={scheduleRoundOpen}
            onOpenChange={setScheduleRoundOpen}
            onScheduled={bump}
          />
          <TemplateBuilderDialog
            target={templateTarget}
            onClose={() => setTemplateTarget(null)}
            onSaved={() => {
              setTemplateTarget(null);
              bump();
            }}
          />
          <InspectionScheduleDialog
            target={scheduleTarget}
            onClose={() => setScheduleTarget(null)}
            onSaved={() => {
              setScheduleTarget(null);
              bump();
            }}
          />
        </>
      ) : null}
    </ModulePageShell>
  );
}

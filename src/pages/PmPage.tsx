import {
  AlertTriangle,
  CalendarCheck,
  CalendarClock,
  CalendarDays,
  CalendarRange,
  ClipboardCheck,
  Gauge,
  List,
  Plus,
  ShieldCheck,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import {
  FilterToggleButton,
  RefreshButton,
  type ViewOption,
  ViewToggle,
} from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { KpiTile } from "@/components/metrics/KpiTile";
import { PmArchivePanel } from "@/components/pm/PmArchivePanel";
import { PmAutopilotStrip } from "@/components/pm/PmAutopilotStrip";
import { PmCalendarView } from "@/components/pm/PmCalendarView";
import { PmDueQueueView } from "@/components/pm/PmDueQueueView";
import { PmExecutionDialog } from "@/components/pm/PmExecutionDialog";
import { PmMeterReadingDialog } from "@/components/pm/PmMeterReadingDialog";
import { PmOccurrenceTransitionDialog } from "@/components/pm/PmOccurrenceTransitionDialog";
import { PmPlanDetailDialog } from "@/components/pm/PmPlanDetailDialog";
import { PmPlanFormDialog } from "@/components/pm/PmPlanFormDialog";
import { PmPlansView } from "@/components/pm/PmPlansView";
import { PmRefreshForecastDialog } from "@/components/pm/PmRefreshForecastDialog";
import { PmVersionFormDialog } from "@/components/pm/PmVersionFormDialog";
import {
  PM_ACTIVE_OCCURRENCE_STATUSES,
  PM_LIFECYCLE_STATUSES,
  PM_OCCURRENCE_STATUSES,
  PM_STRATEGY_TYPES,
} from "@/components/pm/pm-labels";
import {
  type PmDueWindow,
  filterCalendarOccurrences,
  filterDueOccurrences,
  filterPlans,
} from "@/components/pm/pm-model";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfLayout } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { type PmOccurrenceRow, getPmOccurrence } from "@/services/pm-service";
import { pushAppToast } from "@/store/app-toast-store";
import { usePmStore } from "@/stores/pm-store";
import type { PmPlanVersion } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type PmViewMode = "plans" | "due" | "calendar";

/** Periods offered for the PM compliance tile (days back from today). */
const COMPLIANCE_PERIODS = [30, 90, 365] as const;

function isPmViewMode(value: string | null): value is PmViewMode {
  return value === "plans" || value === "due" || value === "calendar";
}

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Preference only; the page works without it.
  }
}

type PendingDelete =
  | { kind: "plan"; id: number; rowVersion: number; code: string }
  | { kind: "version"; version: PmPlanVersion };

export function PmPage() {
  const { t } = useTranslation("pm");
  const { formatNumber, formatPercent } = useFormatters();
  const [searchParams, setSearchParams] = useSearchParams();

  const plans = usePmStore((s) => s.plans);
  const versions = usePmStore((s) => s.versions);
  const occurrences = usePmStore((s) => s.occurrences);
  const dueOccurrences = usePmStore((s) => s.dueOccurrences);
  const calendarOccurrences = usePmStore((s) => s.calendarOccurrences);
  const executions = usePmStore((s) => s.executions);
  const findingsByExecutionId = usePmStore((s) => s.findingsByExecutionId);
  const recurringFindings = usePmStore((s) => s.recurringFindings);
  const planningReadiness = usePmStore((s) => s.planningReadiness);
  const governanceKpis = usePmStore((s) => s.governanceKpis);
  const metrics = usePmStore((s) => s.metrics);
  const activePlanId = usePmStore((s) => s.activePlanId);
  const loading = usePmStore((s) => s.loading);
  const detailLoading = usePmStore((s) => s.detailLoading);
  const saving = usePmStore((s) => s.saving);
  const error = usePmStore((s) => s.error);
  const detailError = usePmStore((s) => s.detailError);
  const loadWorkspace = usePmStore((s) => s.loadWorkspace);
  const openPlan = usePmStore((s) => s.openPlan);
  const closePlan = usePmStore((s) => s.closePlan);
  const createPlan = usePmStore((s) => s.createPlan);
  const updatePlan = usePmStore((s) => s.updatePlan);
  const transitionPlanLifecycle = usePmStore((s) => s.transitionPlanLifecycle);
  const createVersion = usePmStore((s) => s.createVersion);
  const updateVersion = usePmStore((s) => s.updateVersion);
  const publishVersion = usePmStore((s) => s.publishVersion);
  const deletePlan = usePmStore((s) => s.deletePlan);
  const deleteVersion = usePmStore((s) => s.deleteVersion);
  const generateOccurrences = usePmStore((s) => s.generateOccurrences);
  const transitionOccurrence = usePmStore((s) => s.transitionOccurrence);
  const executeOccurrence = usePmStore((s) => s.executeOccurrence);
  const loadFindings = usePmStore((s) => s.loadFindings);
  const compliance = usePmStore((s) => s.compliance);
  const complianceDays = usePmStore((s) => s.complianceDays);
  const loadCompliance = usePmStore((s) => s.loadCompliance);
  const reviseFromFinding = usePmStore((s) => s.reviseFromFinding);

  const [view, setView] = useState<PmViewMode>(() => {
    const raw = readStorage("pm-view-mode");
    return isPmViewMode(raw) ? raw : "due";
  });
  const [showFilters, setShowFilters] = useState(() => readStorage("pm-show-filters") !== "0");
  const [searchInput, setSearchInput] = useState("");
  const [settledSearch, setSettledSearch] = useState("");
  const [lifecycleFilter, setLifecycleFilter] = useState<string | null>(null);
  const [strategyFilter, setStrategyFilter] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [dueWindow, setDueWindow] = useState<PmDueWindow>("all");

  const [planFormOpen, setPlanFormOpen] = useState(false);
  const [planFormMode, setPlanFormMode] = useState<"create" | "edit">("create");
  const [versionFormOpen, setVersionFormOpen] = useState(false);
  const [versionFormMode, setVersionFormMode] = useState<"create" | "edit">("create");
  const [editingVersion, setEditingVersion] = useState<PmPlanVersion | null>(null);
  const [refreshOpen, setRefreshOpen] = useState(false);
  const [transitionOcc, setTransitionOcc] = useState<PmOccurrenceRow | null>(null);
  const [executeOcc, setExecuteOcc] = useState<PmOccurrenceRow | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(null);
  const [focusOccurrenceId, setFocusOccurrenceId] = useState<number | null>(null);
  const [meterReadingOpen, setMeterReadingOpen] = useState(false);

  const activePlan = useMemo(
    () => plans.find((p) => p.id === activePlanId) ?? null,
    [plans, activePlanId],
  );

  useEffect(() => {
    void loadWorkspace();
  }, [loadWorkspace]);

  const refreshCompliance = useCallback(
    (days?: number) => {
      loadCompliance(days).catch(() => {
        // Reported by the global IPC toast; the tile shows "—".
      });
    },
    [loadCompliance],
  );

  useEffect(() => {
    refreshCompliance();
  }, [refreshCompliance]);

  // Notification deep link: `/pm?occurrence=<id>` opens that occurrence in its plan.
  useEffect(() => {
    const raw = searchParams.get("occurrence");
    if (!raw) return;
    const id = Number(raw);
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("occurrence");
        return next;
      },
      { replace: true },
    );
    if (!Number.isFinite(id) || id <= 0) return;
    getPmOccurrence(id)
      .then((occ) => {
        setFocusOccurrenceId(occ.id);
        return openPlan(occ.pm_plan_id);
      })
      .catch(() => {
        // Reported by the global IPC toast (occurrence not found or no access).
      });
  }, [searchParams, openPlan, setSearchParams]);

  useEffect(() => {
    const raw = searchParams.get("planId");
    if (!raw) return;
    const id = Number(raw);
    if (!Number.isFinite(id) || id <= 0) return;
    void openPlan(id);
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("planId");
        return next;
      },
      { replace: true },
    );
  }, [searchParams, openPlan, setSearchParams]);

  const setViewMode = useCallback((mode: PmViewMode) => {
    setView(mode);
    writeStorage("pm-view-mode", mode);
  }, []);

  const toggleFilters = useCallback(() => {
    setShowFilters((prev) => {
      const next = !prev;
      writeStorage("pm-show-filters", next ? "1" : "0");
      return next;
    });
  }, []);

  /** A due KPI tile opens the due queue filtered on the figure behind it. */
  const openDueList = useCallback(
    (window: PmDueWindow, status: string | null) => {
      setViewMode("due");
      setDueWindow(window);
      setStatusFilter(status);
      setStrategyFilter(null);
      setSearchInput("");
      setSettledSearch("");
      setShowFilters(true);
      writeStorage("pm-show-filters", "1");
    },
    [setViewMode],
  );

  const filteredPlans = useMemo(
    () =>
      filterPlans(
        plans.filter((p) => p.lifecycle_status !== "retired"),
        {
          search: settledSearch,
          lifecycle: lifecycleFilter,
          strategy: strategyFilter,
        },
      ),
    [plans, settledSearch, lifecycleFilter, strategyFilter],
  );

  const filteredDue = useMemo(
    () =>
      filterDueOccurrences(dueOccurrences, {
        search: settledSearch,
        status: statusFilter,
        strategy: strategyFilter,
        dueWindow,
      }),
    [dueOccurrences, settledSearch, statusFilter, strategyFilter, dueWindow],
  );

  const filteredCalendar = useMemo(
    () =>
      filterCalendarOccurrences(calendarOccurrences, {
        search: settledSearch,
        status: statusFilter,
        strategy: strategyFilter,
      }),
    [calendarOccurrences, settledSearch, statusFilter, strategyFilter],
  );

  const strategyFilterDef: SmartFilterDef = useMemo(
    () => ({
      id: "strategy",
      label: t("fields.strategy"),
      kind: "select",
      value: strategyFilter,
      options: PM_STRATEGY_TYPES.map((s) => ({ value: s, label: t(`strategy.${s}`) })),
      onChange: setStrategyFilter,
    }),
    [t, strategyFilter],
  );

  const filterDefs: SmartFilterDef[] = useMemo(() => {
    if (view === "plans") {
      return [
        {
          id: "lifecycle",
          label: t("fields.lifecycle"),
          kind: "select",
          value: lifecycleFilter,
          options: PM_LIFECYCLE_STATUSES.filter((s) => s !== "retired").map((s) => ({
            value: s,
            label: t(`lifecycle.${s}`),
          })),
          onChange: setLifecycleFilter,
        },
        strategyFilterDef,
      ];
    }
    const statuses = view === "due" ? PM_ACTIVE_OCCURRENCE_STATUSES : PM_OCCURRENCE_STATUSES;
    const defs: SmartFilterDef[] = [
      {
        id: "status",
        label: t("fields.status"),
        kind: "select",
        value: statusFilter,
        options: statuses.map((s) => ({ value: s, label: t(`occurrenceStatus.${s}`) })),
        onChange: setStatusFilter,
      },
      strategyFilterDef,
    ];
    if (view === "due") {
      defs.push({
        id: "dueWindow",
        label: t("fields.dueWindow"),
        kind: "select",
        value: dueWindow === "all" ? null : dueWindow,
        options: [
          { value: "overdue", label: t("dueWindow.overdue") },
          { value: "today", label: t("dueWindow.today") },
          { value: "next7", label: t("dueWindow.next7") },
        ],
        onChange: (v) => setDueWindow((v as PmDueWindow | null) ?? "all"),
      });
    }
    return defs;
  }, [view, t, lifecycleFilter, statusFilter, dueWindow, strategyFilterDef]);

  const activeFilterCount = (
    view === "plans"
      ? [lifecycleFilter, strategyFilter]
      : view === "due"
        ? [statusFilter, strategyFilter, dueWindow === "all" ? null : dueWindow]
        : [statusFilter, strategyFilter]
  ).filter(Boolean).length;

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setSettledSearch("");
    setLifecycleFilter(null);
    setStrategyFilter(null);
    setStatusFilter(null);
    setDueWindow("all");
  }, []);

  const viewOptions = useMemo<ViewOption<PmViewMode>[]>(
    () => [
      { value: "plans", label: t("views.plans"), icon: List },
      { value: "due", label: t("views.due"), icon: CalendarClock },
      { value: "calendar", label: t("views.calendar"), icon: CalendarDays },
    ],
    [t],
  );

  const count =
    view === "plans"
      ? filteredPlans.length
      : view === "calendar"
        ? filteredCalendar.length
        : filteredDue.length;

  const showListError = error != null;

  const confirmDelete = async () => {
    if (!pendingDelete) return;
    if (pendingDelete.kind === "plan") {
      await deletePlan(pendingDelete.id, pendingDelete.rowVersion);
      pushAppToast({
        title: t("messages.planDeleted", { code: pendingDelete.code }),
        variant: "success",
      });
    } else {
      await deleteVersion(pendingDelete.version.id, pendingDelete.version.row_version);
      pushAppToast({
        title: t("messages.versionDeleted", { version: pendingDelete.version.version_no }),
        variant: "success",
      });
    }
  };

  return (
    <ModulePageShell
      icon={CalendarClock}
      title={t("page.title")}
      count={count}
      bodyClassName={null}
      actions={
        <>
          <PermissionGate permission={P.PM_CREATE}>
            <Button
              className={mfLayout.primaryAction}
              onClick={() => {
                setPlanFormMode("create");
                setPlanFormOpen(true);
              }}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("actions.newPlan")}
            </Button>
          </PermissionGate>
          <PermissionGate permission={P.PM_CREATE}>
            <Button variant="secondary" className="min-h-11" onClick={() => setRefreshOpen(true)}>
              {t("actions.refreshForecast")}
            </Button>
          </PermissionGate>
          <PermissionGate permission={P.PM_EDIT}>
            <Button
              variant="outline"
              className="min-h-11"
              onClick={() => setMeterReadingOpen(true)}
            >
              <Gauge className="h-4 w-4" aria-hidden />
              {t("actions.recordReading")}
            </Button>
          </PermissionGate>
          <ViewToggle value={view} options={viewOptions} onChange={setViewMode} />
          <FilterToggleButton
            open={showFilters}
            onToggle={toggleFilters}
            activeCount={activeFilterCount}
          />
          <RefreshButton
            onRefresh={() => {
              void loadWorkspace();
              refreshCompliance();
            }}
            loading={loading}
          />
        </>
      }
      filterBar={
        <>
          <PmAutopilotStrip />
          <div className="grid gap-3 px-4 pt-3 sm:grid-cols-2 xl:grid-cols-5">
            <KpiTile
              label={t("metrics.overdue")}
              value={metrics ? formatNumber(metrics.overdue_count) : "—"}
              unit={t("metrics.unit")}
              hint={t("metrics.overdueHint")}
              icon={AlertTriangle}
              {...(metrics && metrics.overdue_count > 0 ? { tone: "danger" as const } : {})}
              onOpen={() => openDueList("overdue", null)}
            />
            <KpiTile
              label={t("metrics.dueToday")}
              value={metrics ? formatNumber(metrics.due_today_count) : "—"}
              unit={t("metrics.unit")}
              hint={t("metrics.dueTodayHint")}
              icon={CalendarCheck}
              onOpen={() => openDueList("today", null)}
            />
            <KpiTile
              label={t("metrics.next7d")}
              value={metrics ? formatNumber(metrics.due_next_7d_count) : "—"}
              unit={t("metrics.unit")}
              hint={t("metrics.next7dHint")}
              icon={CalendarRange}
              onOpen={() => openDueList("next7", null)}
            />
            <KpiTile
              label={t("metrics.ready")}
              value={metrics ? formatNumber(metrics.ready_for_scheduling_count) : "—"}
              unit={t("metrics.unit")}
              hint={t("metrics.readyHint")}
              icon={ClipboardCheck}
              onOpen={() => openDueList("all", "ready_for_scheduling")}
            />
            <div className="grid gap-1">
              <KpiTile
                label={t("metrics.compliance")}
                value={
                  compliance?.compliance_pct != null
                    ? formatPercent(compliance.compliance_pct)
                    : "—"
                }
                hint={
                  compliance
                    ? t("metrics.complianceHint", {
                        onTime: formatNumber(compliance.on_time_count),
                        due: formatNumber(compliance.due_count),
                      })
                    : t("metrics.complianceDefinition")
                }
                icon={ShieldCheck}
                {...(compliance?.compliance_pct != null && compliance.late_count > 0
                  ? { tone: "warning" as const }
                  : {})}
              />
              <Label htmlFor="pm-compliance-period" className="sr-only">
                {t("metrics.compliancePeriod")}
              </Label>
              <Select
                value={String(complianceDays)}
                onValueChange={(v) => refreshCompliance(Number(v))}
              >
                <SelectTrigger id="pm-compliance-period" className="h-9 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMPLIANCE_PERIODS.map((days) => (
                    <SelectItem key={days} value={String(days)}>
                      {t("metrics.lastDays", { count: days })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          {showFilters ? (
            <div className="px-4 pt-3">
              <SmartFilterBar
                searchPlaceholder={
                  view === "plans" ? t("page.searchPlaceholder") : t("page.dueSearchPlaceholder")
                }
                searchValue={searchInput}
                onSearchInputChange={setSearchInput}
                onSearchChange={setSettledSearch}
                filters={filterDefs}
                resultCount={count}
                onReset={resetFilters}
              />
            </div>
          ) : null}
        </>
      }
    >
      <div className="flex h-full min-h-0 flex-col">
        <div className="min-h-0 flex-1 overflow-hidden">
          {showListError ? (
            <div className="p-4">
              <ErrorState
                error={error}
                title={t("page.loadFailed")}
                onRetry={() => void loadWorkspace()}
                retrying={loading}
              />
            </div>
          ) : view === "plans" ? (
            <PmPlansView
              plans={filteredPlans}
              loading={loading}
              onOpenPlan={(id) => void openPlan(id)}
            />
          ) : view === "calendar" ? (
            <PmCalendarView
              occurrences={filteredCalendar}
              loading={loading}
              onChipClick={(occ) => void openPlan(occ.pm_plan_id)}
            />
          ) : (
            <PmDueQueueView
              occurrences={filteredDue}
              loading={loading}
              onOpenPlan={(id) => void openPlan(id)}
              onTransition={setTransitionOcc}
              onExecute={setExecuteOcc}
            />
          )}
        </div>
        <PmArchivePanel plans={plans} onOpenPlan={(id) => void openPlan(id)} />
      </div>

      <PmPlanDetailDialog
        open={activePlanId != null && activePlan != null}
        plan={activePlan}
        versions={versions}
        occurrences={occurrences}
        executions={executions}
        findingsByExecutionId={findingsByExecutionId}
        recurringFindings={recurringFindings}
        planningReadiness={planningReadiness}
        governanceKpis={governanceKpis}
        saving={saving}
        detailLoading={detailLoading}
        detailError={detailError}
        onRetryDetail={() => {
          if (activePlanId != null) void openPlan(activePlanId);
        }}
        onClose={() => {
          setFocusOccurrenceId(null);
          closePlan();
        }}
        onEditPlan={() => {
          setPlanFormMode("edit");
          setPlanFormOpen(true);
        }}
        onDeletePlan={() => {
          if (!activePlan) return;
          setPendingDelete({
            kind: "plan",
            id: activePlan.id,
            rowVersion: activePlan.row_version,
            code: activePlan.code,
          });
        }}
        onLifecycle={(next) => {
          if (!activePlan) return;
          transitionPlanLifecycle({
            plan_id: activePlan.id,
            expected_row_version: activePlan.row_version,
            next_status: next,
          })
            .then(() => pushAppToast({ title: t("messages.lifecycleUpdated"), variant: "success" }))
            .catch(() => {
              // Reported by the global IPC toast; the plan keeps its status.
            });
        }}
        onNewVersion={() => {
          setVersionFormMode("create");
          setEditingVersion(null);
          setVersionFormOpen(true);
        }}
        onEditVersion={(version) => {
          setVersionFormMode("edit");
          setEditingVersion(version);
          setVersionFormOpen(true);
        }}
        onPublishVersion={(version) => {
          publishVersion(version.id, version.row_version)
            .then(() =>
              pushAppToast({
                title: t("messages.versionPublished", { version: version.version_no }),
                variant: "success",
              }),
            )
            .catch(() => {
              // Reported by the global IPC toast; the version stays a draft.
            });
        }}
        onDeleteVersion={(version) => setPendingDelete({ kind: "version", version })}
        onTransitionOccurrence={setTransitionOcc}
        onExecuteOccurrence={setExecuteOcc}
        onLoadFindings={(id) => void loadFindings(id)}
        focusOccurrenceId={focusOccurrenceId}
        onReviseFromFinding={async (finding) => {
          if (activePlanId == null) return;
          const draft = await reviseFromFinding(finding.id, activePlanId);
          pushAppToast({
            title: t("messages.revisionCreated", { version: draft.version_no }),
            variant: "success",
          });
        }}
      />

      <PmMeterReadingDialog
        open={meterReadingOpen}
        onClose={() => setMeterReadingOpen(false)}
        onRecorded={() => {
          pushAppToast({ title: t("messages.readingRecorded"), variant: "success" });
          void loadWorkspace();
        }}
      />

      <ConfirmDialog
        open={pendingDelete != null}
        onOpenChange={(next) => {
          if (!next) setPendingDelete(null);
        }}
        title={
          pendingDelete?.kind === "version"
            ? t("dialogs.deleteVersion.title")
            : t("dialogs.deletePlan.title")
        }
        description={
          pendingDelete?.kind === "version"
            ? t("dialogs.deleteVersion.description", {
                version: pendingDelete.version.version_no,
              })
            : t("dialogs.deletePlan.description", {
                code: pendingDelete?.kind === "plan" ? pendingDelete.code : "",
              })
        }
        confirmLabel={
          pendingDelete?.kind === "version" ? t("actions.deleteVersion") : t("actions.deletePlan")
        }
        destructive
        onConfirm={confirmDelete}
      />

      <PmPlanFormDialog
        open={planFormOpen}
        mode={planFormMode}
        plan={planFormMode === "edit" ? activePlan : null}
        saving={saving}
        onClose={() => setPlanFormOpen(false)}
        onCreate={async (input) => {
          await createPlan(input);
        }}
        onUpdate={async (planId, rowVersion, input) => {
          await updatePlan(planId, rowVersion, input);
        }}
      />

      {activePlan ? (
        <PmVersionFormDialog
          open={versionFormOpen}
          mode={versionFormMode}
          plan={activePlan}
          version={editingVersion}
          saving={saving}
          onClose={() => {
            setVersionFormOpen(false);
            setEditingVersion(null);
          }}
          onCreate={async (pmPlanId, input) => {
            await createVersion(pmPlanId, input);
          }}
          onUpdate={async (versionId, rowVersion, input) => {
            await updateVersion(versionId, rowVersion, input);
          }}
        />
      ) : null}

      <PmRefreshForecastDialog
        open={refreshOpen}
        plans={plans}
        activePlanId={activePlanId}
        saving={saving}
        onClose={() => setRefreshOpen(false)}
        onRun={async (input) => {
          const result = await generateOccurrences(input);
          pushAppToast({
            title:
              result.generated_count > 0
                ? t("messages.generationCreated", { count: result.generated_count })
                : t("messages.generationNoNew"),
            variant: "success",
          });
        }}
      />

      <PmOccurrenceTransitionDialog
        open={transitionOcc != null}
        occurrence={transitionOcc}
        saving={saving}
        onClose={() => setTransitionOcc(null)}
        onSubmit={async (input) => {
          await transitionOccurrence(input);
          pushAppToast({ title: t("messages.occurrenceUpdated"), variant: "success" });
        }}
      />

      <PmExecutionDialog
        open={executeOcc != null}
        occurrence={executeOcc}
        saving={saving}
        onClose={() => setExecuteOcc(null)}
        onSubmit={async (input) => {
          await executeOccurrence(input);
          pushAppToast({ title: t("messages.executionLogged"), variant: "success" });
        }}
      />
    </ModulePageShell>
  );
}

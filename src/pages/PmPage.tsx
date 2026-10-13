import { CalendarClock, CalendarDays, Filter, List, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { PmArchivePanel } from "@/components/pm/PmArchivePanel";
import { PmAutopilotStrip } from "@/components/pm/PmAutopilotStrip";
import { PmCalendarView } from "@/components/pm/PmCalendarView";
import { PmDueQueueView } from "@/components/pm/PmDueQueueView";
import { PmExecutionDialog } from "@/components/pm/PmExecutionDialog";
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
  filterCalendarOccurrences,
  filterDueOccurrences,
  filterPlans,
  type PmDueWindow,
} from "@/components/pm/pm-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfLayout } from "@/design-system/tokens";
import { usePmStore } from "@/stores/pm-store";
import type { PmOccurrence, PmPlanVersion } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type PmViewMode = "plans" | "due" | "calendar";

function isPmViewMode(value: string | null): value is PmViewMode {
  return value === "plans" || value === "due" || value === "calendar";
}

export function PmPage() {
  const { t } = useTranslation("pm");
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
  const activePlanId = usePmStore((s) => s.activePlanId);
  const loading = usePmStore((s) => s.loading);
  const saving = usePmStore((s) => s.saving);
  const error = usePmStore((s) => s.error);
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

  const [view, setView] = useState<PmViewMode>(() => {
    const raw = localStorage.getItem("pm-view-mode");
    return isPmViewMode(raw) ? raw : "due";
  });
  const [showFilters, setShowFilters] = useState(
    () => localStorage.getItem("pm-show-filters") !== "0",
  );
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
  const [transitionOcc, setTransitionOcc] = useState<PmOccurrence | null>(null);
  const [executeOcc, setExecuteOcc] = useState<PmOccurrence | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const activePlan = useMemo(
    () => plans.find((p) => p.id === activePlanId) ?? null,
    [plans, activePlanId],
  );

  useEffect(() => {
    void loadWorkspace();
  }, [loadWorkspace]);

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
    localStorage.setItem("pm-view-mode", mode);
  }, []);

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

  const planFilterDefs: SmartFilterDef[] = useMemo(
    () => [
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
      {
        id: "strategy",
        label: t("fields.strategy"),
        kind: "select",
        value: strategyFilter,
        options: PM_STRATEGY_TYPES.map((s) => ({
          value: s,
          label: t(`strategy.${s}`),
        })),
        onChange: setStrategyFilter,
      },
    ],
    [t, lifecycleFilter, strategyFilter],
  );

  const dueFilterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "status",
        label: t("fields.status"),
        kind: "select",
        value: statusFilter,
        options: PM_ACTIVE_OCCURRENCE_STATUSES.map((s) => ({
          value: s,
          label: t(`occurrenceStatus.${s}`),
        })),
        onChange: setStatusFilter,
      },
      {
        id: "strategy",
        label: t("fields.strategy"),
        kind: "select",
        value: strategyFilter,
        options: PM_STRATEGY_TYPES.map((s) => ({
          value: s,
          label: t(`strategy.${s}`),
        })),
        onChange: setStrategyFilter,
      },
      {
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
      },
    ],
    [t, statusFilter, strategyFilter, dueWindow],
  );

  const calendarFilterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "status",
        label: t("fields.status"),
        kind: "select",
        value: statusFilter,
        options: PM_OCCURRENCE_STATUSES.map((s) => ({
          value: s,
          label: t(`occurrenceStatus.${s}`),
        })),
        onChange: setStatusFilter,
      },
      {
        id: "strategy",
        label: t("fields.strategy"),
        kind: "select",
        value: strategyFilter,
        options: PM_STRATEGY_TYPES.map((s) => ({
          value: s,
          label: t(`strategy.${s}`),
        })),
        onChange: setStrategyFilter,
      },
    ],
    [t, statusFilter, strategyFilter],
  );

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setSettledSearch("");
    setLifecycleFilter(null);
    setStrategyFilter(null);
    setStatusFilter(null);
    setDueWindow("all");
  }, []);

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <CalendarClock className={mfLayout.moduleHeaderIcon} />
          <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
          <Badge variant="secondary">
            {view === "plans"
              ? t("page.planCount", { count: filteredPlans.length })
              : view === "calendar"
                ? t("page.calendarCount", { count: filteredCalendar.length })
                : t("page.dueCount", { count: filteredDue.length })}
          </Badge>
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          <PermissionGate anyOf={[P.PM_CREATE, P.PM_EDIT]}>
            <Button
              size="sm"
              onClick={() => {
                setPlanFormMode("create");
                setPlanFormOpen(true);
              }}
            >
              <Plus className="mr-1 h-4 w-4" />
              {t("actions.newPlan")}
            </Button>
          </PermissionGate>
          <PermissionGate anyOf={[P.PM_CREATE, P.PM_EDIT]}>
            <Button size="sm" variant="secondary" onClick={() => setRefreshOpen(true)}>
              {t("actions.refreshForecast")}
            </Button>
          </PermissionGate>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              const next = !showFilters;
              setShowFilters(next);
              localStorage.setItem("pm-show-filters", next ? "1" : "0");
            }}
          >
            <Filter className="mr-1 h-4 w-4" />
            {t("page.filters")}
          </Button>
          <Button size="sm" variant="outline" onClick={() => void loadWorkspace()}>
            <RefreshCw className="mr-1 h-4 w-4" />
            {t("actions.refresh")}
          </Button>
          <div className={mfLayout.viewToggleGroup}>
            <Button
              variant={view === "plans" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => setViewMode("plans")}
              title={t("views.plans")}
            >
              <List className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "due" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => setViewMode("due")}
              title={t("views.due")}
            >
              <CalendarClock className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "calendar" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => setViewMode("calendar")}
              title={t("views.calendar")}
            >
              <CalendarDays className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </div>

      <PmAutopilotStrip />

      {error ? (
        <div className="mx-4 mt-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      ) : null}
      {notice ? (
        <div className="mx-4 mt-3 rounded-md border border-border bg-muted/40 px-3 py-2 text-sm">
          {notice}
        </div>
      ) : null}

      {showFilters ? (
        <div className="px-4 pt-3">
          <SmartFilterBar
            searchPlaceholder={
              view === "plans" ? t("page.searchPlaceholder") : t("page.dueSearchPlaceholder")
            }
            searchValue={searchInput}
            onSearchInputChange={setSearchInput}
            onSearchChange={setSettledSearch}
            filters={
              view === "plans"
                ? planFilterDefs
                : view === "calendar"
                  ? calendarFilterDefs
                  : dueFilterDefs
            }
            resultCount={
              view === "plans"
                ? filteredPlans.length
                : view === "calendar"
                  ? filteredCalendar.length
                  : filteredDue.length
            }
            onReset={resetFilters}
          />
        </div>
      ) : null}

      <div className={`${mfLayout.moduleWorkspace} flex-col`}>
        <div className="min-h-0 flex-1 overflow-hidden">
          {view === "plans" ? (
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
        onClose={closePlan}
        onEditPlan={() => {
          setPlanFormMode("edit");
          setPlanFormOpen(true);
        }}
        onDeletePlan={() => {
          if (!activePlan) return;
          if (!window.confirm(t("dialogs.deletePlan.description", { code: activePlan.code }))) {
            return;
          }
          void deletePlan(activePlan.id, activePlan.row_version).then(() => {
            setNotice(t("messages.planDeleted", { code: activePlan.code }));
          });
        }}
        onLifecycle={(next) => {
          if (!activePlan) return;
          void transitionPlanLifecycle({
            plan_id: activePlan.id,
            expected_row_version: activePlan.row_version,
            next_status: next,
          }).then(() => setNotice(t("messages.lifecycleUpdated")));
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
          void publishVersion(version.id, version.row_version);
        }}
        onDeleteVersion={(version) => {
          if (
            !window.confirm(t("dialogs.deleteVersion.description", { version: version.version_no }))
          ) {
            return;
          }
          void deleteVersion(version.id, version.row_version).then(() => {
            setNotice(t("messages.versionDeleted", { version: version.version_no }));
          });
        }}
        onTransitionOccurrence={setTransitionOcc}
        onExecuteOccurrence={setExecuteOcc}
        onLoadFindings={(id) => void loadFindings(id)}
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
          setNotice(t("messages.generationStarted"));
          const result = await generateOccurrences(input);
          setNotice(
            result.generated_count > 0
              ? t("messages.generationCreated", { count: result.generated_count })
              : t("messages.generationNoNew"),
          );
        }}
      />

      <PmOccurrenceTransitionDialog
        open={transitionOcc != null}
        occurrence={transitionOcc}
        saving={saving}
        onClose={() => setTransitionOcc(null)}
        onSubmit={async (input) => {
          await transitionOccurrence(input);
          setNotice(t("messages.occurrenceUpdated"));
        }}
      />

      <PmExecutionDialog
        open={executeOcc != null}
        occurrence={executeOcc}
        saving={saving}
        onClose={() => setExecuteOcc(null)}
        onSubmit={async (input) => {
          await executeOccurrence(input);
          setNotice(t("messages.executionLogged"));
        }}
      />
    </div>
  );
}

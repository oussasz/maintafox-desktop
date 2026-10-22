/**
 * Personnel workspace — aligned with OT / DI page shell (PRD §6.6).
 */

import type { ColumnDef } from "@tanstack/react-table";
import { GanttChart, LayoutGrid, List, Plus, Users } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { RefreshButton, ViewToggle } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import {
  AvailabilityCalendar,
  type AvailabilityCalendarHandle,
} from "@/components/personnel/AvailabilityCalendar";
import { ExternalCompaniesPanel } from "@/components/personnel/ExternalCompaniesPanel";
import { PersonnelArchivePanel } from "@/components/personnel/PersonnelArchivePanel";
import { PersonnelCard } from "@/components/personnel/PersonnelCard";
import { PersonnelCreateDialog } from "@/components/personnel/PersonnelCreateDialog";
import { PersonnelDetailWorkspace } from "@/components/personnel/PersonnelDetailWorkspace";
import { PersonnelExportMenu } from "@/components/personnel/PersonnelExportMenu";
import { PersonnelImportWizard } from "@/components/personnel/PersonnelImportWizard";
import { PersonnelStatusReviewNotice } from "@/components/personnel/PersonnelStatusReviewNotice";
import {
  TeamCapacityBoard,
  type TeamCapacityBoardHandle,
} from "@/components/personnel/TeamCapacityBoard";
import { WorkCalendarPanel } from "@/components/personnel/WorkCalendarPanel";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import type { AvailabilityViewMode } from "@/components/personnel/availability-model";
import { usePersonnelLists } from "@/components/personnel/use-personnel-lists";
import { Button } from "@/components/ui/button";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { ReviewMark } from "@/components/ui/review-mark";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { type Tone, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import { listPositions } from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";
import type { Personnel } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type PersonnelViewMode = "list" | "cards";
type PersonnelTab = "directory" | "availability" | "teams" | "companies" | "calendar" | "analytics";

const AVAILABILITY_CODES = [
  "available",
  "assigned",
  "in_training",
  "on_leave",
  "blocked",
  "inactive",
] as const;

const EMPLOYMENT_ORIGIN_CODES = ["internal", "external"] as const;

const VIEW_STORAGE_KEY = "personnel-view-mode";

/** List/table badge styling — same pattern as DI / OT status chips */
const AVAILABILITY_TONE: Record<string, Tone> = {
  available: "success",
  assigned: "progress",
  in_training: "info",
  on_leave: "neutral",
  blocked: "danger",
  inactive: "neutral",
};

export function PersonnelPage() {
  const { t } = useTranslation("personnel");

  const items = usePersonnelStore((s) => s.items);
  const total = usePersonnelStore((s) => s.total);
  const loading = usePersonnelStore((s) => s.loading);
  const loadError = usePersonnelStore((s) => s.loadError);
  const detailError = usePersonnelStore((s) => s.detailError);
  const loadingMore = usePersonnelStore((s) => s.loadingMore);
  const loadMore = usePersonnelStore((s) => s.loadMore);
  const filter = usePersonnelStore((s) => s.filter);
  const setFilter = usePersonnelStore((s) => s.setFilter);
  const loadPersonnel = usePersonnelStore((s) => s.loadPersonnel);
  const openPersonnel = usePersonnelStore((s) => s.openPersonnel);
  const openCreateForm = usePersonnelStore((s) => s.openCreateForm);
  const activePersonnel = usePersonnelStore((s) => s.activePersonnel);

  const [view, setView] = useState<PersonnelViewMode>(() => {
    try {
      return localStorage.getItem(VIEW_STORAGE_KEY) === "cards" ? "cards" : "list";
    } catch {
      return "list";
    }
  });
  const [availabilityView, setAvailabilityView] = useState<AvailabilityViewMode>("chronology");
  const [searchParams] = useSearchParams();
  const requestedTab = searchParams.get("tab");
  const requestedTeam = Number(searchParams.get("team"));
  const openTeamId = Number.isInteger(requestedTeam) && requestedTeam > 0 ? requestedTeam : null;
  const [activeTab, setActiveTab] = useState<PersonnelTab>(
    requestedTab === "teams" || requestedTab === "calendar" || requestedTab === "companies"
      ? requestedTab
      : "directory",
  );
  const [searchInput, setSearchInput] = useState("");
  const isDirectoryTab = activeTab === "directory";
  const isAvailabilityTab = activeTab === "availability";
  const isTeamsTab = activeTab === "teams";
  const availabilityRef = useRef<AvailabilityCalendarHandle>(null);
  const teamsRef = useRef<TeamCapacityBoardHandle>(null);

  const { can } = usePermissions();
  // Employment types and statuses come from the plant's lists (Données de référence).
  const lists = usePersonnelLists();
  // The position filter is offered only to users who may read positions (per.position.view).
  const canViewPositions = can(P.PER_POSITION_VIEW);
  // The Analyses tab is three per.report reports: no tab without that right.
  const canViewReports = can(P.PER_REPORT);
  const [positions, setPositions] = useState<{ id: number; code: string; name: string }[]>([]);
  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);
  const [lookupError, setLookupError] = useState<unknown>(null);

  const loadLookups = useCallback(async () => {
    setLookupError(null);
    const [pos, snap] = await Promise.allSettled([
      canViewPositions ? listPositions() : Promise.resolve([]),
      getOrgDesignerSnapshot(),
    ]);
    if (pos.status === "fulfilled") {
      setPositions(
        pos.value
          .filter((p) => p.is_active !== 0)
          .map((p) => ({ id: p.id, code: p.code, name: p.name })),
      );
    } else {
      setPositions([]);
      setLookupError(pos.reason);
    }
    if (snap.status === "fulfilled") {
      const nodes = snap.value.nodes.filter((n) => n.status === "active");
      const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
      setEntityNodes(
        entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work),
      );
    } else {
      setEntityNodes([]);
      setLookupError(snap.reason);
    }
  }, [canViewPositions]);

  useEffect(() => {
    void loadPersonnel();
  }, [loadPersonnel]);

  useEffect(() => {
    void loadLookups();
  }, [loadLookups]);

  const onSearchChange = useCallback(
    (val: string) => {
      setFilter({ search: val.trim() || null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const switchView = useCallback((v: PersonnelViewMode) => {
    setView(v);
    try {
      localStorage.setItem(VIEW_STORAGE_KEY, v);
    } catch {
      // Remembering the view is a convenience only.
    }
  }, []);

  const selectedStatuses = useMemo(
    () => filter.availability_status ?? [],
    [filter.availability_status],
  );
  const selectedEmployment = useMemo(() => filter.employment_type ?? [], [filter.employment_type]);
  const selectedOrigin = useMemo(() => filter.employment_origin ?? [], [filter.employment_origin]);
  const selectedEmpStatus = useMemo(
    () => filter.employment_status ?? [],
    [filter.employment_status],
  );

  const handleEntityFilter = useCallback(
    (val: string | null) => {
      setFilter({ entity_id: val ? Number(val) : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handlePositionFilter = useCallback(
    (val: string | null) => {
      setFilter({ position_id: val ? Number(val) : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handleStatusMulti = useCallback(
    (next: string[]) => {
      setFilter({ availability_status: next.length ? next : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handleEmploymentMulti = useCallback(
    (next: string[]) => {
      setFilter({ employment_type: next.length ? next : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handleOriginMulti = useCallback(
    (next: string[]) => {
      setFilter({ employment_origin: next.length ? next : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handleEmpStatusMulti = useCallback(
    (next: string[]) => {
      setFilter({ employment_status: next.length ? next : null });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const clearAllFilters = useCallback(() => {
    setSearchInput("");
    setFilter({
      search: null,
      entity_id: null,
      position_id: null,
      availability_status: null,
      employment_type: null,
      employment_origin: null,
      employment_status: null,
    });
    void loadPersonnel();
  }, [setFilter, loadPersonnel]);

  const entityValue = filter.entity_id != null ? String(filter.entity_id) : null;
  const positionValue = filter.position_id != null ? String(filter.position_id) : null;

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "entity",
        kind: "select",
        label: t("filters.entity"),
        options: entityNodes.map((n) => ({
          value: String(n.node_id),
          label: `${n.code} — ${n.name}`,
        })),
        value: entityValue,
        onChange: handleEntityFilter,
      },
      ...(canViewPositions
        ? [
            {
              id: "position",
              kind: "select" as const,
              label: t("filters.position"),
              options: positions.map((p) => ({
                value: String(p.id),
                label: `${p.code} — ${p.name}`,
              })),
              value: positionValue,
              onChange: handlePositionFilter,
            },
          ]
        : []),
      {
        id: "status",
        kind: "multi-select",
        label: t("filters.status"),
        options: AVAILABILITY_CODES.map((code) => ({
          value: code,
          label: t(`status.${code}`),
        })),
        value: selectedStatuses,
        onChange: handleStatusMulti,
      },
      {
        id: "employment",
        kind: "multi-select",
        label: t("filters.employmentType"),
        options: lists.values("employment_types").map((v) => ({
          value: v.code,
          label: v.review_required
            ? `${lists.text("employment_types", v.code)} · ${t("common:listValue.review")}`
            : lists.text("employment_types", v.code),
        })),
        value: selectedEmployment,
        onChange: handleEmploymentMulti,
      },
      {
        id: "origin",
        kind: "multi-select",
        label: t("filters.employmentOrigin", "Origin"),
        options: EMPLOYMENT_ORIGIN_CODES.map((code) => ({
          value: code,
          label: t(`employmentOrigin.${code}`, code),
        })),
        value: selectedOrigin,
        onChange: handleOriginMulti,
      },
      {
        id: "empStatus",
        kind: "multi-select",
        label: t("filters.employmentStatus", "Status"),
        options: lists.values("employment_statuses").map((v) => ({
          value: v.code,
          label: v.review_required
            ? `${lists.text("employment_statuses", v.code)} · ${t("common:listValue.review")}`
            : lists.text("employment_statuses", v.code),
        })),
        value: selectedEmpStatus,
        onChange: handleEmpStatusMulti,
      },
    ],
    [
      t,
      lists,
      canViewPositions,
      entityNodes,
      positions,
      entityValue,
      positionValue,
      selectedStatuses,
      selectedEmployment,
      selectedOrigin,
      selectedEmpStatus,
      handleEntityFilter,
      handlePositionFilter,
      handleStatusMulti,
      handleEmploymentMulti,
      handleOriginMulti,
      handleEmpStatusMulti,
    ],
  );

  const columns: ColumnDef<Personnel>[] = useMemo(
    () => [
      {
        accessorKey: "employee_code",
        header: t("list.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.employee_code}</span>,
      },
      {
        accessorKey: "full_name",
        header: t("list.columns.fullName"),
        cell: ({ row }) => (
          <span className="max-w-[200px] truncate font-medium">{row.original.full_name}</span>
        ),
      },
      {
        accessorKey: "position_name",
        header: t("list.columns.position"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.position_name ?? "—"}</span>
        ),
      },
      {
        accessorKey: "entity_name",
        header: t("list.columns.entity"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.entity_name ?? "—"}</span>
        ),
      },
      {
        accessorKey: "team_name",
        header: t("list.columns.team"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.team_name ?? "—"}</span>
        ),
      },
      {
        accessorKey: "availability_status",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.availability_status;
          return <StatusChip label={t(`status.${s}`)} tone={toneOf(AVAILABILITY_TONE, s)} dot />;
        },
      },
      {
        accessorKey: "employment_type",
        header: t("list.columns.employmentType"),
        cell: ({ row }) => {
          const code = row.original.employment_type;
          const value = lists.option("employment_types", code);
          return (
            <span className="inline-flex items-center gap-1 text-xs text-text-muted">
              {lists.text("employment_types", code)}
              {lists.canRead && !lists.loading && (value == null || value.review_required) ? (
                <ReviewMark />
              ) : null}
            </span>
          );
        },
      },
      {
        accessorKey: "schedule_name",
        header: t("list.columns.schedule"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.schedule_name ?? "—"}</span>
        ),
      },
    ],
    [t, lists],
  );

  /** One refresh button: it reloads what the open tab shows. */
  const refreshActiveTab = () => {
    if (isDirectoryTab) void loadPersonnel();
    else if (isAvailabilityTab) availabilityRef.current?.refresh();
    else if (isTeamsTab) teamsRef.current?.refresh();
  };
  const canRefreshTab = isDirectoryTab || isAvailabilityTab || isTeamsTab;

  const headerActions = (
    <>
      {isDirectoryTab ? (
        <>
          <PermissionGate permission={P.PER_MANAGE}>
            <Button className={mfLayout.primaryAction} onClick={() => openCreateForm()}>
              <Plus className="h-4 w-4" aria-hidden />
              {t("action.create")}
            </Button>
          </PermissionGate>
          <PersonnelImportWizard />
          <PersonnelExportMenu />
          <ViewToggle
            value={view}
            onChange={switchView}
            options={[
              { value: "list", label: t("view.list"), icon: List },
              { value: "cards", label: t("view.cards"), icon: LayoutGrid },
            ]}
          />
        </>
      ) : null}

      {isAvailabilityTab ? (
        <>
          <PermissionGate permission={P.PER_MANAGE}>
            <Button
              className={mfLayout.primaryAction}
              onClick={() => availabilityRef.current?.openCreateBlock()}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("availability.actions.createBlock")}
            </Button>
          </PermissionGate>
          <ViewToggle
            value={availabilityView}
            onChange={setAvailabilityView}
            options={[
              { value: "chronology", label: t("availability.views.chronology"), icon: GanttChart },
              { value: "list", label: t("availability.views.list"), icon: List },
            ]}
          />
        </>
      ) : null}

      {isTeamsTab ? (
        <PermissionGate permission={P.PER_MANAGE}>
          <Button className={mfLayout.primaryAction} onClick={() => teamsRef.current?.openCreate()}>
            <Plus className="h-4 w-4" aria-hidden />
            {t("crews.actions.create")}
          </Button>
        </PermissionGate>
      ) : null}

      {canRefreshTab ? (
        <RefreshButton onRefresh={refreshActiveTab} loading={isDirectoryTab && loading} />
      ) : null}
    </>
  );

  return (
    <ModulePageShell
      icon={Users}
      title={t("page.title")}
      count={loadError != null && items.length === 0 ? null : total}
      actions={headerActions}
      bodyClassName={null}
    >
      {/* ── Tabs → tab-owned filters/content ─ */}
      <div className={mfLayout.moduleWorkspaceBody}>
        <div>
          <div>
            <Tabs
              value={activeTab}
              onValueChange={(v) => setActiveTab(v as PersonnelTab)}
              className="flex min-h-0 w-full flex-1 flex-col"
            >
              <TabsList className="grid w-full max-w-3xl grid-cols-2 sm:grid-cols-4">
                <TabsTrigger value="directory">{t("tabs.directory")}</TabsTrigger>
                <TabsTrigger value="availability">{t("tabs.availability")}</TabsTrigger>
                <TabsTrigger value="teams">{t("tabs.teams")}</TabsTrigger>
                <TabsTrigger value="companies">{t("tabs.companies")}</TabsTrigger>
                <TabsTrigger value="calendar">{t("tabs.calendar")}</TabsTrigger>
                {canViewReports ? (
                  <TabsTrigger value="analytics">{t("tabs.analytics")}</TabsTrigger>
                ) : null}
              </TabsList>

              <TabsContent
                value="directory"
                className="mt-4 flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden"
              >
                <SmartFilterBar
                  searchPlaceholder={t("filters.searchPlaceholder")}
                  searchValue={searchInput}
                  onSearchInputChange={setSearchInput}
                  onSearchChange={onSearchChange}
                  filters={filterDefs}
                  resultCount={total}
                  onReset={clearAllFilters}
                />
                <PersonnelStatusReviewNotice
                  className="mx-3 mb-2"
                  refreshKey={items}
                  onOpenPersonnel={(id) => void openPersonnel(id)}
                />
                {lists.error != null ? (
                  <ErrorState
                    className="mx-3 mb-2"
                    error={lists.error}
                    title={t("page.listsLoadError")}
                    onRetry={() => void lists.reload()}
                  />
                ) : null}
                {lookupError != null ? (
                  <ErrorState
                    className="mx-3 mb-2"
                    error={lookupError}
                    title={t("page.filterLookupError")}
                    onRetry={() => void loadLookups()}
                  />
                ) : null}
                <div className={mfLayout.moduleWorkspaceSplit}>
                  <div className="flex w-[55%] min-w-[400px] flex-col border-r border-surface-border">
                    <div className="min-h-0 flex-1 overflow-auto p-1">
                      {loadError != null && !loading ? (
                        <ErrorState
                          className="mb-2"
                          error={loadError}
                          title={t("page.loadError")}
                          onRetry={() => void (items.length > 0 ? loadMore() : loadPersonnel())}
                        />
                      ) : null}
                      {loadError != null && items.length === 0 ? null : (
                        <TruncationNotice
                          className="mb-2"
                          shown={items.length}
                          total={total}
                          onLoadMore={() => void loadMore()}
                          loadingMore={loadingMore}
                        />
                      )}
                      {loadError != null && items.length === 0
                        ? null
                        : view === "list" && (
                            <DataTable
                              columns={columns}
                              data={items}
                              searchable={false}
                              pageSize={20}
                              isLoading={loading}
                              skeletonRows={8}
                              onRowClick={(row) => void openPersonnel(row.id)}
                              isRowSelected={(row) => row.id === activePersonnel?.personnel.id}
                            />
                          )}
                      {loadError != null && items.length === 0
                        ? null
                        : view === "cards" && (
                            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
                              {loading
                                ? Array.from({ length: 6 }, (_, i) => (
                                    <div
                                      key={i}
                                      className="h-48 animate-pulse rounded-lg border border-surface-border bg-muted/40"
                                    />
                                  ))
                                : items.map((p) => (
                                    <PersonnelCard
                                      key={p.id}
                                      personnel={p}
                                      onViewDetails={() => void openPersonnel(p.id)}
                                    />
                                  ))}
                            </div>
                          )}
                    </div>
                  </div>
                  <div className="min-w-[300px] flex-1 overflow-hidden">
                    {detailError != null ? (
                      <div className="p-4">
                        <ErrorState error={detailError} title={t("detail.loadError")} />
                      </div>
                    ) : activePersonnel ? (
                      <PersonnelDetailWorkspace detail={activePersonnel} />
                    ) : (
                      <div className="flex h-full items-center justify-center p-6">
                        <p className="text-sm text-text-muted">{t("detail.noSelection")}</p>
                      </div>
                    )}
                  </div>
                </div>
              </TabsContent>

              <TabsContent value="availability" className="mt-4 flex min-h-0 flex-1 flex-col">
                <AvailabilityCalendar
                  ref={availabilityRef}
                  entityId={filter.entity_id ?? null}
                  viewMode={availabilityView}
                  onViewModeChange={setAvailabilityView}
                  onOpenPersonnel={(personnelId) => {
                    setActiveTab("directory");
                    void openPersonnel(personnelId);
                  }}
                />
              </TabsContent>

              <TabsContent value="teams" className="mt-4">
                <TeamCapacityBoard
                  ref={teamsRef}
                  entityId={filter.entity_id ?? null}
                  openTeamId={openTeamId}
                  onOpenPersonnel={(personnelId) => {
                    // The person's record lives in the directory tab: show it there.
                    setActiveTab("directory");
                    void openPersonnel(personnelId);
                  }}
                />
              </TabsContent>

              <TabsContent value="companies" className="mt-4">
                <ExternalCompaniesPanel />
              </TabsContent>

              <TabsContent value="calendar" className="mt-4">
                <WorkCalendarPanel
                  entities={entityNodes.map((n) => ({ id: n.node_id, name: n.name }))}
                />
              </TabsContent>

              {canViewReports ? (
                <TabsContent value="analytics" className="mt-4">
                  <div className="flex flex-col gap-4">
                    <WorkforceReportPanel />
                  </div>
                </TabsContent>
              ) : null}
            </Tabs>
          </div>
        </div>
      </div>

      <PersonnelArchivePanel />

      <PersonnelCreateDialog />
    </ModulePageShell>
  );
}

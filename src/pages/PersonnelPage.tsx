/**
 * Personnel workspace — aligned with OT / DI page shell (PRD §6.6).
 */

import type { ColumnDef } from "@tanstack/react-table";
import { GanttChart, LayoutGrid, List, Plus, RefreshCw, Users } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
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
import {
  TeamCapacityBoard,
  type TeamCapacityBoardHandle,
} from "@/components/personnel/TeamCapacityBoard";
import { WorkCalendarPanel } from "@/components/personnel/WorkCalendarPanel";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import type { AvailabilityViewMode } from "@/components/personnel/availability-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { type Tone, toneOf } from "@/design-system/tone";
import { cn } from "@/lib/utils";
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

const EMPLOYMENT_CODES = ["employee", "contractor", "temp", "vendor"] as const;
const EMPLOYMENT_ORIGIN_CODES = ["internal", "external"] as const;
const EMPLOYMENT_STATUS_CODES = ["active", "inactive", "suspended", "terminated"] as const;

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
  const error = usePersonnelStore((s) => s.error);
  const filter = usePersonnelStore((s) => s.filter);
  const setFilter = usePersonnelStore((s) => s.setFilter);
  const loadPersonnel = usePersonnelStore((s) => s.loadPersonnel);
  const openPersonnel = usePersonnelStore((s) => s.openPersonnel);
  const openCreateForm = usePersonnelStore((s) => s.openCreateForm);
  const activePersonnel = usePersonnelStore((s) => s.activePersonnel);

  const [view, setView] = useState<PersonnelViewMode>(
    () => (localStorage.getItem(VIEW_STORAGE_KEY) as PersonnelViewMode) || "list",
  );
  const [availabilityView, setAvailabilityView] = useState<AvailabilityViewMode>("chronology");
  const [searchParams] = useSearchParams();
  const requestedTab = searchParams.get("tab");
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

  const [positions, setPositions] = useState<{ id: number; code: string; name: string }[]>([]);
  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);

  const loadLookups = useCallback(async () => {
    try {
      const [pos, snap] = await Promise.all([listPositions(), getOrgDesignerSnapshot()]);
      setPositions(
        pos.filter((p) => p.is_active !== 0).map((p) => ({ id: p.id, code: p.code, name: p.name })),
      );
      const nodes = snap.nodes.filter((n) => n.status === "active");
      const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
      setEntityNodes(
        entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work),
      );
    } catch {
      setPositions([]);
      setEntityNodes([]);
    }
  }, []);

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
    localStorage.setItem(VIEW_STORAGE_KEY, v);
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
      {
        id: "position",
        kind: "select",
        label: t("filters.position"),
        options: positions.map((p) => ({
          value: String(p.id),
          label: `${p.code} — ${p.name}`,
        })),
        value: positionValue,
        onChange: handlePositionFilter,
      },
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
        options: EMPLOYMENT_CODES.map((code) => ({
          value: code,
          label: t(`employmentType.${code}`),
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
        options: EMPLOYMENT_STATUS_CODES.map((code) => ({
          value: code,
          label: t(`employmentStatus.${code}`, code),
        })),
        value: selectedEmpStatus,
        onChange: handleEmpStatusMulti,
      },
    ],
    [
      t,
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
          const et = row.original.employment_type as "employee" | "contractor" | "temp" | "vendor";
          return <span className="text-xs text-text-muted">{t(`employmentType.${et}`)}</span>;
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
    [t],
  );

  return (
    <div className={mfLayout.moduleRoot}>
      {/* ── Page header (same shell as DI / OT) ───────────────────────── */}
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Users className={mfLayout.moduleHeaderIcon} aria-hidden />
          <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className={mfLayout.moduleHeaderActions}>
          {isDirectoryTab ? (
            <>
              <PermissionGate permission={P.PER_MANAGE}>
                <Button size="sm" onClick={() => openCreateForm()} className="gap-1.5">
                  <Plus className="h-3.5 w-3.5" />
                  {t("action.create")}
                </Button>
              </PermissionGate>
              <PersonnelImportWizard />
              <PersonnelExportMenu />

              <div className={mfLayout.viewToggleGroup}>
                <Button
                  variant={view === "list" ? "default" : "ghost"}
                  size="sm"
                  className={mfLayout.viewToggleButton}
                  onClick={() => switchView("list")}
                  title={t("view.list")}
                >
                  <List className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant={view === "cards" ? "default" : "ghost"}
                  size="sm"
                  className={mfLayout.viewToggleButton}
                  onClick={() => switchView("cards")}
                  title={t("view.cards")}
                >
                  <LayoutGrid className="h-3.5 w-3.5" />
                </Button>
              </div>
            </>
          ) : null}

          {isAvailabilityTab ? (
            <>
              <PermissionGate permission={P.PER_MANAGE}>
                <Button
                  size="sm"
                  className="gap-1.5"
                  onClick={() => availabilityRef.current?.openCreateBlock()}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("availability.actions.createBlock")}
                </Button>
              </PermissionGate>
              <div className={mfLayout.viewToggleGroup}>
                <Button
                  variant={availabilityView === "chronology" ? "default" : "ghost"}
                  size="sm"
                  className={mfLayout.viewToggleButton}
                  onClick={() => setAvailabilityView("chronology")}
                  title={t("availability.views.chronology")}
                  aria-label={t("availability.views.chronology")}
                >
                  <GanttChart className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant={availabilityView === "list" ? "default" : "ghost"}
                  size="sm"
                  className={mfLayout.viewToggleButton}
                  onClick={() => setAvailabilityView("list")}
                  title={t("availability.views.list")}
                  aria-label={t("availability.views.list")}
                >
                  <List className="h-3.5 w-3.5" />
                </Button>
              </div>
            </>
          ) : null}

          {isDirectoryTab ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => void loadPersonnel()}
              disabled={loading}
              className="gap-1.5"
              title={t("action.refresh")}
            >
              <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
            </Button>
          ) : null}

          {isAvailabilityTab ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => availabilityRef.current?.refresh()}
              className="gap-1.5"
              title={t("availability.actions.refresh")}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          ) : null}

          {isTeamsTab ? (
            <PermissionGate permission={P.PER_MANAGE}>
              <Button size="sm" className="gap-1.5" onClick={() => teamsRef.current?.openCreate()}>
                <Plus className="h-3.5 w-3.5" />
                {t("crews.actions.create")}
              </Button>
            </PermissionGate>
          ) : null}

          {isTeamsTab ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => teamsRef.current?.refresh()}
              className="gap-1.5"
              title={t("action.refresh")}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          ) : null}
        </div>
      </div>

      {error ? (
        <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive">
          {error}
        </div>
      ) : null}

      {/* ── Main workspace: Header → Tabs → tab-owned filters/content ─ */}
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          <div className={mfLayout.moduleWorkspaceBody}>
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
                <TabsTrigger value="analytics">{t("tabs.analytics")}</TabsTrigger>
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
                <div className={mfLayout.moduleWorkspaceSplit}>
                  <div className="flex w-[55%] min-w-[400px] flex-col border-r border-surface-border">
                    <div className="min-h-0 flex-1 overflow-auto p-1">
                      {view === "list" && (
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
                      {view === "cards" && (
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
                    {activePersonnel ? (
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
                  onOpenPersonnel={(personnelId) => {
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

              <TabsContent value="analytics" className="mt-4">
                <div className="flex flex-col gap-4">
                  <WorkforceReportPanel />
                </div>
              </TabsContent>
            </Tabs>
          </div>
        </div>
      </div>

      <PersonnelArchivePanel />

      <PersonnelCreateDialog />
    </div>
  );
}

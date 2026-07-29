/**
 * Personnel workspace — aligned with OT / DI page shell (PRD §6.6).
 */

import type { ColumnDef } from "@tanstack/react-table";
import { LayoutGrid, List, Plus, RefreshCw, Users } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { AvailabilityCalendar } from "@/components/personnel/AvailabilityCalendar";
import { PersonnelArchivePanel } from "@/components/personnel/PersonnelArchivePanel";
import { PersonnelCard } from "@/components/personnel/PersonnelCard";
import { PersonnelCreateDialog } from "@/components/personnel/PersonnelCreateDialog";
import { PersonnelDetailDialog } from "@/components/personnel/PersonnelDetailDialog";
import { PersonnelExportMenu } from "@/components/personnel/PersonnelExportMenu";
import { PersonnelImportWizard } from "@/components/personnel/PersonnelImportWizard";
import { SkillsMatrixPanel } from "@/components/personnel/SkillsMatrixPanel";
import { TeamCapacityBoard } from "@/components/personnel/TeamCapacityBoard";
import { TrainingQualificationPanel } from "@/components/personnel/TrainingQualificationPanel";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import { listPositions } from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";
import type { Personnel } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type PersonnelViewMode = "list" | "cards";

const AVAILABILITY_CODES = [
  "available",
  "assigned",
  "in_training",
  "on_leave",
  "blocked",
  "inactive",
] as const;

const EMPLOYMENT_CODES = ["employee", "contractor", "temp", "vendor"] as const;

const VIEW_STORAGE_KEY = "personnel-view-mode";

/** List/table badge styling — same pattern as DI / OT status chips */
const AVAILABILITY_TABLE_STYLE: Record<string, string> = {
  available: "bg-emerald-100 text-emerald-800",
  assigned: "bg-blue-100 text-blue-800",
  in_training: "bg-amber-100 text-amber-900",
  on_leave: "bg-gray-100 text-gray-700",
  blocked: "bg-red-100 text-red-800",
  inactive: "bg-slate-100 text-slate-700",
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

  const [view, setView] = useState<PersonnelViewMode>(
    () => (localStorage.getItem(VIEW_STORAGE_KEY) as PersonnelViewMode) || "list",
  );
  const [searchInput, setSearchInput] = useState("");

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

  const clearAllFilters = useCallback(() => {
    setSearchInput("");
    setFilter({
      search: null,
      entity_id: null,
      position_id: null,
      availability_status: null,
      employment_type: null,
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
        allLabel: t("filters.all"),
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
        allLabel: t("filters.all"),
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
    ],
    [
      t,
      entityNodes,
      positions,
      entityValue,
      positionValue,
      selectedStatuses,
      selectedEmployment,
      handleEntityFilter,
      handlePositionFilter,
      handleStatusMulti,
      handleEmploymentMulti,
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
          return (
            <Badge
              variant="outline"
              className={cn(
                "text-[10px] border-0",
                AVAILABILITY_TABLE_STYLE[s] ?? "bg-gray-100 text-gray-600",
              )}
            >
              {t(`status.${s}`)}
            </Badge>
          );
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
        </div>
      </div>

      <SmartFilterBar
        searchPlaceholder={t("filters.searchPlaceholder")}
        searchValue={searchInput}
        onSearchInputChange={setSearchInput}
        onSearchChange={onSearchChange}
        filters={filterDefs}
        resultCount={total}
        onReset={clearAllFilters}
      />

      {error ? (
        <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive">
          {error}
        </div>
      ) : null}

      {/* ── Main workspace ───────────────────────────────────────────── */}
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          <div className={mfLayout.moduleWorkspaceBody}>
            <Tabs defaultValue="directory" className="w-full">
              <TabsList className="grid w-full max-w-4xl grid-cols-2 sm:grid-cols-3 lg:grid-cols-5">
                <TabsTrigger value="directory">{t("tabs.directory")}</TabsTrigger>
                <TabsTrigger value="skills">{t("tabs.skillsMatrix")}</TabsTrigger>
                <TabsTrigger value="availability">{t("tabs.availabilityCalendar")}</TabsTrigger>
                <TabsTrigger value="capacity">{t("tabs.teamCapacity")}</TabsTrigger>
                <TabsTrigger value="training">{t("tabs.training")}</TabsTrigger>
              </TabsList>

              <TabsContent value="directory" className="mt-4">
                <div className="mb-4">
                  <WorkforceReportPanel />
                </div>
                {view === "list" && (
                  <DataTable
                    columns={columns}
                    data={items}
                    searchable={false}
                    pageSize={20}
                    isLoading={loading}
                    skeletonRows={8}
                    onRowClick={(row) => void openPersonnel(row.id)}
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
              </TabsContent>

              <TabsContent value="skills" className="mt-4">
                <SkillsMatrixPanel
                  entityId={filter.entity_id ?? null}
                  teamId={filter.team_id ?? null}
                />
              </TabsContent>

              <TabsContent value="availability" className="mt-4">
                <AvailabilityCalendar
                  entityId={filter.entity_id ?? null}
                  teamId={filter.team_id ?? null}
                />
              </TabsContent>

              <TabsContent value="capacity" className="mt-4">
                <TeamCapacityBoard entityId={filter.entity_id ?? null} />
              </TabsContent>

              <TabsContent value="training" className="mt-4">
                <TrainingQualificationPanel />
              </TabsContent>
            </Tabs>
          </div>
        </div>
      </div>

      <PersonnelArchivePanel />

      <PersonnelCreateDialog />
      <PersonnelDetailDialog />
    </div>
  );
}

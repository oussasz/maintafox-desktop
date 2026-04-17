/**
 * Personnel workspace — aligned with OT / DI page shell (PRD §6.6).
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, LayoutGrid, List, Plus, RefreshCw, Search, Users, X } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PersonnelArchivePanel } from "@/components/personnel/PersonnelArchivePanel";
import { AvailabilityCalendar } from "@/components/personnel/AvailabilityCalendar";
import { PersonnelCard } from "@/components/personnel/PersonnelCard";
import { PersonnelCreateDialog } from "@/components/personnel/PersonnelCreateDialog";
import { PersonnelDetailDialog } from "@/components/personnel/PersonnelDetailDialog";
import { PersonnelExportMenu } from "@/components/personnel/PersonnelExportMenu";
import { PersonnelImportWizard } from "@/components/personnel/PersonnelImportWizard";
import { SkillsMatrixPanel } from "@/components/personnel/SkillsMatrixPanel";
import { TeamCapacityBoard } from "@/components/personnel/TeamCapacityBoard";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import { getOrgDesignerSnapshot } from "@/services/org-designer-service";
import { listPositions } from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";
import type { Personnel } from "@shared/ipc-types";

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
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [positions, setPositions] = useState<{ id: number; code: string; name: string }[]>([]);
  const [entityNodes, setEntityNodes] = useState<OrgDesignerNodeRow[]>([]);

  const loadLookups = useCallback(async () => {
    try {
      const [pos, snap] = await Promise.all([listPositions(), getOrgDesignerSnapshot()]);
      setPositions(pos.filter((p) => p.is_active !== 0).map((p) => ({ id: p.id, code: p.code, name: p.name })));
      const nodes = snap.nodes.filter((n) => n.status === "active");
      const entityCandidates = nodes.filter((n) => n.active_binding_count > 0);
      setEntityNodes(entityCandidates.length > 0 ? entityCandidates : nodes.filter((n) => n.can_own_work));
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

  const handleSearchChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const val = e.target.value;
      setSearchInput(val);
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
      searchTimerRef.current = setTimeout(() => {
        setFilter({ search: val.trim() || null });
        void loadPersonnel();
      }, 300);
    },
    [setFilter, loadPersonnel],
  );

  const clearSearch = useCallback(() => {
    setSearchInput("");
    setFilter({ search: null });
    void loadPersonnel();
  }, [setFilter, loadPersonnel]);

  const switchView = useCallback((v: PersonnelViewMode) => {
    setView(v);
    localStorage.setItem(VIEW_STORAGE_KEY, v);
  }, []);

  const selectedStatuses = filter.availability_status ?? [];
  const selectedEmployment = filter.employment_type ?? [];

  const toggleStatus = useCallback(
    (code: string, checked: boolean) => {
      const cur = filter.availability_status ?? [];
      const next = checked ? [...new Set([...cur, code])] : cur.filter((c) => c !== code);
      setFilter({ availability_status: next.length ? next : null });
      void loadPersonnel();
    },
    [filter.availability_status, setFilter, loadPersonnel],
  );

  const toggleEmployment = useCallback(
    (code: string, checked: boolean) => {
      const cur = filter.employment_type ?? [];
      const next = checked ? [...new Set([...cur, code])] : cur.filter((c) => c !== code);
      setFilter({ employment_type: next.length ? next : null });
      void loadPersonnel();
    },
    [filter.employment_type, setFilter, loadPersonnel],
  );

  const handleEntityFilter = useCallback(
    (val: string) => {
      setFilter({ entity_id: val === "__all__" ? null : Number(val) });
      void loadPersonnel();
    },
    [setFilter, loadPersonnel],
  );

  const handlePositionFilter = useCallback(
    (val: string) => {
      setFilter({ position_id: val === "__all__" ? null : Number(val) });
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
        cell: ({ row }) => <span className="max-w-[200px] truncate font-medium">{row.original.full_name}</span>,
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

  const entityValue =
    filter.entity_id != null && filter.entity_id !== undefined ? String(filter.entity_id) : "__all__";
  const positionValue =
    filter.position_id != null && filter.position_id !== undefined ? String(filter.position_id) : "__all__";

  return (
    <div className="flex h-full flex-col">
      {/* ── Page header (same shell as DI / OT) ───────────────────────── */}
      <div className="flex items-center justify-between border-b border-surface-border px-6 py-3">
        <div className="flex items-center gap-3">
          <Users className="h-5 w-5 text-text-muted" aria-hidden />
          <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          <PermissionGate permission="per.manage">
            <Button size="sm" onClick={() => openCreateForm()} className="gap-1.5">
              <Plus className="h-3.5 w-3.5" />
              {t("action.create")}
            </Button>
          </PermissionGate>
          <PersonnelImportWizard />
          <PersonnelExportMenu />

          <div className="flex items-center gap-0.5 rounded-md border p-0.5">
            <Button
              variant={view === "list" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("list")}
              title={t("view.list")}
            >
              <List className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "cards" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
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

      {/* ── Filters ─────────────────────────────────────────────────── */}
      <div className="flex flex-wrap items-center gap-2 border-b border-surface-border px-6 py-2">
        <div className="relative max-w-sm flex-1">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            className="h-8 pl-9 text-sm"
            placeholder={t("filters.searchPlaceholder")}
            value={searchInput}
            onChange={handleSearchChange}
          />
          {searchInput ? (
            <button
              type="button"
              className="absolute right-2 top-2 text-text-muted hover:text-text-primary"
              onClick={clearSearch}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </div>

        <Select value={entityValue} onValueChange={handleEntityFilter}>
          <SelectTrigger className="h-8 w-[200px] text-sm">
            <SelectValue placeholder={t("filters.entity")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">{t("filters.all")}</SelectItem>
            {entityNodes.map((n) => (
              <SelectItem key={n.node_id} value={String(n.node_id)}>
                {n.code} — {n.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select value={positionValue} onValueChange={handlePositionFilter}>
          <SelectTrigger className="h-8 w-[200px] text-sm">
            <SelectValue placeholder={t("filters.position")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">{t("filters.all")}</SelectItem>
            {positions.map((p) => (
              <SelectItem key={p.id} value={String(p.id)}>
                {p.code} — {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 min-w-[160px] justify-between text-sm font-normal"
            >
              {t("filters.status")}
              <ChevronDown className="h-3.5 w-3.5 opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-56" align="start">
            <DropdownMenuLabel>{t("filters.status")}</DropdownMenuLabel>
            <DropdownMenuSeparator />
            {AVAILABILITY_CODES.map((code) => (
              <DropdownMenuCheckboxItem
                key={code}
                checked={selectedStatuses.includes(code)}
                onCheckedChange={(c) => toggleStatus(code, Boolean(c))}
                onSelect={(e) => e.preventDefault()}
              >
                {t(`status.${code}`)}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-8 min-w-[180px] justify-between text-sm font-normal"
            >
              {t("filters.employmentType")}
              <ChevronDown className="h-3.5 w-3.5 opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-56" align="start">
            <DropdownMenuLabel>{t("filters.employmentType")}</DropdownMenuLabel>
            <DropdownMenuSeparator />
            {EMPLOYMENT_CODES.map((code) => (
              <DropdownMenuCheckboxItem
                key={code}
                checked={selectedEmployment.includes(code)}
                onCheckedChange={(c) => toggleEmployment(code, Boolean(c))}
                onSelect={(e) => e.preventDefault()}
              >
                {t(`employmentType.${code}`)}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <Button type="button" variant="ghost" size="sm" className="h-8 text-sm" onClick={clearAllFilters}>
          {t("filters.clearAll")}
        </Button>
      </div>

      {error ? (
        <div className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive">
          {error}
        </div>
      ) : null}

      {/* ── Main workspace ───────────────────────────────────────────── */}
      <div className="flex min-h-0 flex-1 flex-col overflow-auto p-4">
        <Tabs defaultValue="directory" className="w-full">
          <TabsList className="grid w-full max-w-2xl grid-cols-4">
            <TabsTrigger value="directory">{t("tabs.directory")}</TabsTrigger>
            <TabsTrigger value="skills">{t("tabs.skillsMatrix")}</TabsTrigger>
            <TabsTrigger value="availability">{t("tabs.availabilityCalendar")}</TabsTrigger>
            <TabsTrigger value="capacity">{t("tabs.teamCapacity")}</TabsTrigger>
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
            <SkillsMatrixPanel entityId={filter.entity_id ?? null} teamId={filter.team_id ?? null} />
          </TabsContent>

          <TabsContent value="availability" className="mt-4">
            <AvailabilityCalendar entityId={filter.entity_id ?? null} teamId={filter.team_id ?? null} />
          </TabsContent>

          <TabsContent value="capacity" className="mt-4">
            <TeamCapacityBoard entityId={filter.entity_id ?? null} />
          </TabsContent>
        </Tabs>
      </div>

      <PersonnelArchivePanel />

      <PersonnelCreateDialog />
      <PersonnelDetailDialog />
    </div>
  );
}

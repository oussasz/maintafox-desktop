/**
 * WorkOrdersPage.tsx
 *
 * Multi-view work order (OT) workspace.
 * Views: List, Kanban (placeholder), Calendar (placeholder), Dashboard (placeholder).
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import type { ColumnDef } from "@tanstack/react-table";
import {
  BarChart3,
  CalendarDays,
  Columns3,
  Filter,
  List,
  Plus,
  RefreshCw,
  Search,
  Shield,
  Wrench,
  X,
} from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { WoArchivePanel } from "@/components/wo/WoArchivePanel";
import { WoCalendarView } from "@/components/wo/WoCalendarView";
import { WoDashboardView } from "@/components/wo/WoDashboardView";
import { WoDetailDialog } from "@/components/wo/WoDetailDialog";
import { WoDiManagementPanel } from "@/components/wo/WoDiManagementPanel";
import { WoFormDialog } from "@/components/wo/WoFormDialog";
import { WoIntegrityWorkbench } from "@/components/wo/WoIntegrityWorkbench";
import { WoKanbanView } from "@/components/wo/WoKanbanView";
import { mfInput, mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useWoStore } from "@/stores/wo-store";
import { formatDate } from "@/utils/format-date";
import { STATUS_STYLE, statusToI18nKey } from "@/utils/wo-status";
import type { WorkOrder } from "@shared/ipc-types";

type WoViewMode = "list" | "kanban" | "calendar" | "dashboard" | "integrity";

// ── Component ───────────────────────────────────────────────────────────────

export function WorkOrdersPage() {
  const { t, i18n } = useTranslation("ot");
  const items = useWoStore((s) => s.items);
  const total = useWoStore((s) => s.total);
  const loading = useWoStore((s) => s.loading);
  const loadWos = useWoStore((s) => s.loadWos);
  const openWo = useWoStore((s) => s.openWo);
  const activeWo = useWoStore((s) => s.activeWo);
  const closeWo = useWoStore((s) => s.closeActiveWo);
  const openCreateForm = useWoStore((s) => s.openCreateForm);
  const setFilter = useWoStore((s) => s.setFilter);

  const [view, setView] = useState<WoViewMode>(
    () => (localStorage.getItem("wo-view-mode") as WoViewMode) || "kanban",
  );
  const [showFilters, setShowFilters] = useState(
    () => localStorage.getItem("wo-show-filters") !== "0",
  );

  // ── Search with debounce ──────────────────────────────────────────────

  const [searchInput, setSearchInput] = useState("");
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleSearchChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const val = e.target.value;
      setSearchInput(val);
      if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
      searchTimerRef.current = setTimeout(() => {
        setFilter({ search: val || null });
        void loadWos();
      }, 300);
    },
    [setFilter, loadWos],
  );

  const clearSearch = useCallback(() => {
    setSearchInput("");
    setFilter({ search: null });
    void loadWos();
  }, [setFilter, loadWos]);

  // ── Status / type / priority filters ──────────────────────────────

  const STATUS_OPTIONS = useMemo(
    () =>
      Object.keys(STATUS_STYLE).map((code) => ({
        code,
        label: t(`status.${statusToI18nKey(code)}`),
      })),
    [t],
  );

  const TYPE_OPTIONS = useMemo(
    () =>
      [
        "corrective",
        "preventive",
        "predictive",
        "improvement",
        "inspection",
        "overhaul",
        "condition_based",
        "permit",
      ].map((code) => ({
        code,
        label: t(`type.${code === "condition_based" ? "conditionBased" : code}`),
      })),
    [t],
  );

  const PRIORITY_OPTIONS = useMemo(
    () =>
      [1, 2, 3, 4, 5].map((n) => ({
        value: n,
        label: `${n}`,
      })),
    [],
  );

  const [statusFilter, setStatusFilter] = useState<string>("__all__");
  const [typeFilter, setTypeFilter] = useState<string>("__all__");
  const [priorityFilter, setPriorityFilter] = useState<string>("__all__");

  const handleStatusFilter = useCallback(
    (val: string) => {
      setStatusFilter(val);
      setFilter({ status_codes: val === "__all__" ? null : [val] });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handleTypeFilter = useCallback(
    (val: string) => {
      setTypeFilter(val);
      setFilter({ type_codes: val === "__all__" ? null : [val] });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handlePriorityFilter = useCallback(
    (val: string) => {
      setPriorityFilter(val);
      setFilter({ urgency_level: val === "__all__" ? null : Number(val) });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  // ── Load on mount ─────────────────────────────────────────────────────

  useEffect(() => {
    void loadWos();
  }, [loadWos]);

  const switchView = useCallback((v: WoViewMode) => {
    setView(v);
    localStorage.setItem("wo-view-mode", v);
  }, []);

  const toggleFilters = useCallback(() => {
    setShowFilters((prev) => {
      const next = !prev;
      localStorage.setItem("wo-show-filters", next ? "1" : "0");
      return next;
    });
  }, []);

  // ── Columns ───────────────────────────────────────────────────────────

  const columns: ColumnDef<WorkOrder>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.code}</span>,
      },
      {
        accessorKey: "title",
        header: t("list.columns.title"),
        cell: ({ row }) => (
          <span className="max-w-[200px] truncate block">{row.original.title}</span>
        ),
      },
      {
        accessorKey: "equipment_name",
        header: t("list.columns.equipment"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.asset_label ?? "—"}</span>
        ),
      },
      {
        accessorKey: "type_label",
        header: t("list.columns.type"),
        cell: ({ row }) => <span className="text-xs">{row.original.type_label ?? "—"}</span>,
      },
      {
        accessorKey: "status",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.status_code ?? "draft";
          return (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ${STATUS_STYLE[s] ?? "bg-gray-100 text-gray-600"}`}
            >
              {t(`status.${statusToI18nKey(s)}`)}
            </Badge>
          );
        },
      },
      {
        accessorKey: "assigned_to_name",
        header: t("list.columns.assignedTo"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.responsible_username ?? "—"}
          </span>
        ),
      },
      {
        accessorKey: "planned_end",
        header: t("list.columns.plannedEnd"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.planned_end ? formatDate(row.original.planned_end, i18n.language) : "—"}
          </span>
        ),
      },
    ],
    [t, i18n.language],
  );

  return (
    <div className={mfLayout.moduleRoot}>
      {/* ── Page header ──────────────────────────────────────────────── */}
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Wrench className={mfLayout.moduleHeaderIcon} />
          <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className={mfLayout.moduleHeaderActions}>
          {/* New WO button */}
          <PermissionGate permission="ot.create">
            <Button size="sm" onClick={() => openCreateForm()} className="gap-1.5">
              <Plus className="h-3.5 w-3.5" />
              {t("action.create")}
            </Button>
          </PermissionGate>

          {/* View toggle */}
          <div className={mfLayout.viewToggleGroup}>
            <Button
              variant={view === "list" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => switchView("list")}
              title={t("page.viewList")}
            >
              <List className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "kanban" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => switchView("kanban")}
              title={t("page.viewKanban")}
            >
              <Columns3 className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "calendar" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => switchView("calendar")}
              title={t("page.viewCalendar")}
            >
              <CalendarDays className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "dashboard" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => switchView("dashboard")}
              title={t("page.viewDashboard")}
            >
              <BarChart3 className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "integrity" ? "default" : "ghost"}
              size="sm"
              className={mfLayout.viewToggleButton}
              onClick={() => switchView("integrity")}
              title={t("page.viewIntegrity")}
            >
              <Shield className="h-3.5 w-3.5" />
            </Button>
          </div>

          <Button
            variant="outline"
            size="sm"
            onClick={toggleFilters}
            title={t("page.filterSettings")}
            className="gap-1.5"
          >
            <Filter className="h-3.5 w-3.5" />
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadWos()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </div>

      {/* ── Filters ──────────────────────────────────────────────────── */}
      {showFilters && (
        <div className={mfLayout.moduleFilterBar}>
          <div className="relative flex-1 max-w-sm">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
            <Input
              className={mfInput.filterSearch}
              placeholder={t("search.placeholder")}
              value={searchInput}
              onChange={handleSearchChange}
            />
            {searchInput && (
              <button
                type="button"
                className="absolute right-2 top-2 text-text-muted hover:text-text-primary"
                onClick={clearSearch}
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>

          <Select value={statusFilter} onValueChange={handleStatusFilter}>
            <SelectTrigger className={cn(mfInput.filterSelect, "w-[160px]")}>
              <SelectValue placeholder={t("list.filters.status")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">{t("list.filters.status")}</SelectItem>
              {STATUS_OPTIONS.map((opt) => (
                <SelectItem key={opt.code} value={opt.code}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Select value={typeFilter} onValueChange={handleTypeFilter}>
            <SelectTrigger className={cn(mfInput.filterSelect, "w-[160px]")}>
              <SelectValue placeholder={t("list.filters.type")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">{t("list.filters.type")}</SelectItem>
              {TYPE_OPTIONS.map((opt) => (
                <SelectItem key={opt.code} value={opt.code}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Select value={priorityFilter} onValueChange={handlePriorityFilter}>
            <SelectTrigger className={cn(mfInput.filterSelect, "w-[130px]")}>
              <SelectValue placeholder={t("list.filters.priority")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">{t("list.filters.priority")}</SelectItem>
              {PRIORITY_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={String(opt.value)}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* ── DI management panel (ot.edit permission) ─────────────────── */}
      <PermissionGate permission="ot.edit">
        <WoDiManagementPanel />
      </PermissionGate>

      {/* ── Main workspace ───────────────────────────────────────────── */}
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          {view === "integrity" && <WoIntegrityWorkbench />}
          {view === "list" && (
            <div className="p-4">
              <DataTable
                columns={columns}
                data={items}
                searchable
                pageSize={20}
                isLoading={loading}
                skeletonRows={8}
                onRowClick={(row) => void openWo(row.id)}
              />
            </div>
          )}
          {view === "kanban" && (
            <WoKanbanView items={items} onCardClick={(wo) => void openWo(wo.id)} />
          )}
          {view === "calendar" && (
            <WoCalendarView items={items} onSelect={(wo) => void openWo(wo.id)} />
          )}
          {view === "dashboard" && <WoDashboardView />}
        </div>
      </div>

      {/* ── Archive panel ────────────────────────────────────────── */}
      <WoArchivePanel onRowClick={(wo) => void openWo(wo.id)} />

      {/* ── Form dialog ──────────────────────────────────────────────── */}
      <WoFormDialog />

      {/* ── Detail dialog ────────────────────────────────────────────── */}
      <WoDetailDialog wo={activeWo?.wo ?? null} open={activeWo !== null} onClose={closeWo} />
    </div>
  );
}

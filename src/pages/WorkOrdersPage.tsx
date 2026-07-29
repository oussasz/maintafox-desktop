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
  MoreHorizontal,
  Plus,
  RefreshCw,
  Shield,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { WoArchivePanel } from "@/components/wo/WoArchivePanel";
import { WoCalendarView } from "@/components/wo/WoCalendarView";
import { WoCancelDialog } from "@/components/wo/WoCancelDialog";
import { WoContextMenu } from "@/components/wo/WoContextMenu";
import { WoDashboardView } from "@/components/wo/WoDashboardView";
import { WoDetailDialog } from "@/components/wo/WoDetailDialog";
import { WoDiManagementPanel } from "@/components/wo/WoDiManagementPanel";
import { WoFormDialog } from "@/components/wo/WoFormDialog";
import { WoIntegrityWorkbench } from "@/components/wo/WoIntegrityWorkbench";
import { WoKanbanView } from "@/components/wo/WoKanbanView";
import { printWoFiche } from "@/components/wo/WoPrintFiche";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
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
  const detailLoading = useWoStore((s) => s.detailLoading);
  const closeWo = useWoStore((s) => s.closeActiveWo);
  const openCreateForm = useWoStore((s) => s.openCreateForm);
  const setFilter = useWoStore((s) => s.setFilter);

  const [view, setView] = useState<WoViewMode>(
    () => (localStorage.getItem("wo-view-mode") as WoViewMode) || "kanban",
  );
  const [showFilters, setShowFilters] = useState(
    () => localStorage.getItem("wo-show-filters") !== "0",
  );
  const [cancelTarget, setCancelTarget] = useState<WorkOrder | null>(null);
  const woTypes = useWorkOrderTypesCatalog((s) => s.types);
  const loadWoTypes = useWorkOrderTypesCatalog((s) => s.load);
  const woPriorities = useWorkOrderPrioritiesCatalog((s) => s.priorities);
  const loadWoPriorities = useWorkOrderPrioritiesCatalog((s) => s.load);

  // ── Search / filters (SmartFilterBar) ─────────────────────────────────

  const [searchInput, setSearchInput] = useState("");
  const [searchParams, setSearchParams] = useSearchParams();

  useEffect(() => {
    const raw = searchParams.get("openWo");
    if (!raw) return;
    const id = Number(raw);
    if (!Number.isFinite(id) || id <= 0) return;
    void openWo(id);
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("openWo");
        return next;
      },
      { replace: true },
    );
  }, [searchParams, openWo, setSearchParams]);

  const STATUS_OPTIONS = useMemo(
    () =>
      Object.keys(STATUS_STYLE).map((code) => ({
        value: code,
        label: t(`status.${statusToI18nKey(code)}`),
      })),
    [t],
  );

  const TYPE_OPTIONS = useMemo(
    () =>
      woTypes
        .filter((type) => type.is_active)
        .map((type) => ({
          value: type.code,
          label: t(`type.${type.code === "condition_based" ? "conditionBased" : type.code}`, {
            defaultValue: type.label,
          }),
        })),
    [woTypes, t],
  );

  const PRIORITY_OPTIONS = useMemo(
    () =>
      woPriorities
        .filter((p) => p.is_active)
        .map((p) => ({
          value: String(p.level),
          label: i18n.language.toLowerCase().startsWith("fr") ? p.label_fr : p.label,
        })),
    [woPriorities, i18n.language],
  );

  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const [priorityFilter, setPriorityFilter] = useState<string | null>(null);

  const onSearchChange = useCallback(
    (val: string) => {
      setFilter({ search: val.trim() || null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handleStatusFilter = useCallback(
    (val: string | null) => {
      setStatusFilter(val);
      setFilter({ status_codes: val ? [val] : null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handleTypeFilter = useCallback(
    (val: string | null) => {
      setTypeFilter(val);
      setFilter({ type_codes: val ? [val] : null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handlePriorityFilter = useCallback(
    (val: string | null) => {
      setPriorityFilter(val);
      setFilter({ urgency_level: val ? Number(val) : null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setStatusFilter(null);
    setTypeFilter(null);
    setPriorityFilter(null);
    setFilter({
      search: null,
      status_codes: null,
      type_codes: null,
      urgency_level: null,
    });
    void loadWos();
  }, [setFilter, loadWos]);

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "status",
        kind: "select",
        label: t("list.filters.status"),
        options: STATUS_OPTIONS,
        value: statusFilter,
        onChange: handleStatusFilter,
      },
      {
        id: "type",
        kind: "select",
        label: t("list.filters.type"),
        options: TYPE_OPTIONS,
        value: typeFilter,
        onChange: handleTypeFilter,
      },
      {
        id: "priority",
        kind: "select",
        label: t("list.filters.priority"),
        options: PRIORITY_OPTIONS,
        value: priorityFilter,
        onChange: handlePriorityFilter,
      },
    ],
    [
      t,
      STATUS_OPTIONS,
      TYPE_OPTIONS,
      PRIORITY_OPTIONS,
      statusFilter,
      typeFilter,
      priorityFilter,
      handleStatusFilter,
      handleTypeFilter,
      handlePriorityFilter,
    ],
  );

  // ── Load on mount ─────────────────────────────────────────────────────

  useEffect(() => {
    void loadWos();
  }, [loadWos]);

  useEffect(() => {
    void loadWoTypes();
  }, [loadWoTypes]);

  useEffect(() => {
    void loadWoPriorities();
  }, [loadWoPriorities]);

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
      {
        id: "actions",
        header: "",
        cell: ({ row }) => {
          const wo = row.original;
          return (
            <div
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => e.stopPropagation()}
            >
              <WoContextMenu
                wo={wo}
                onViewDetail={(item) => void openWo(item.id)}
                onEdit={(item) => openCreateForm(item)}
                onStart={(item) => void openWo(item.id)}
                onComplete={(item) => void openWo(item.id)}
                onPrint={(item) =>
                  void printWoFiche(item, t, i18n.resolvedLanguage || i18n.language || "fr")
                }
                onCancel={(item) => setCancelTarget(item)}
              >
                <Button variant="ghost" size="sm" className="h-7 w-7 p-0">
                  <MoreHorizontal className="h-4 w-4" />
                  <span className="sr-only">{t("contextMenu.viewDetail")}</span>
                </Button>
              </WoContextMenu>
            </div>
          );
        },
      },
    ],
    [t, i18n.language, i18n.resolvedLanguage, openWo, openCreateForm],
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
        <SmartFilterBar
          searchPlaceholder={t("search.placeholder")}
          searchValue={searchInput}
          onSearchInputChange={setSearchInput}
          onSearchChange={onSearchChange}
          filters={filterDefs}
          resultCount={total}
          onReset={resetFilters}
        />
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
      <WoDetailDialog
        wo={activeWo?.wo ?? null}
        open={activeWo !== null || detailLoading}
        loading={detailLoading && !activeWo}
        onClose={closeWo}
      />

      <WoCancelDialog
        wo={cancelTarget}
        open={cancelTarget !== null}
        onOpenChange={(open) => {
          if (!open) setCancelTarget(null);
        }}
      />
    </div>
  );
}

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
  List,
  MoreHorizontal,
  Plus,
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
import {
  FilterToggleButton,
  RefreshButton,
  type ViewOption,
  ViewToggle,
} from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
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
import { WO_STATUS_TONE, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import { formatDate } from "@/utils/format-date";
import { WO_LIFECYCLE_STATES, isWoLifecycleState } from "@/utils/wo-lifecycle";
import { statusToI18nKey } from "@/utils/wo-status";
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type WoViewMode = "list" | "kanban" | "calendar" | "dashboard" | "integrity";

/** Current lifecycle status codes offered as filters (legacy aliases are display-only). */
const WO_FILTER_STATUSES = [
  "draft",
  "planning",
  "ready",
  "in_progress",
  "on_hold",
  "completed",
  "closed",
  "cancelled",
] as const;

// ── Component ───────────────────────────────────────────────────────────────

export function WorkOrdersPage() {
  const { t, i18n } = useTranslation("ot");
  const items = useWoStore((s) => s.items);
  const total = useWoStore((s) => s.total);
  const loading = useWoStore((s) => s.loading);
  const loadingMore = useWoStore((s) => s.loadingMore);
  const listError = useWoStore((s) => s.listError);
  const loadWos = useWoStore((s) => s.loadWos);
  const loadMoreWos = useWoStore((s) => s.loadMoreWos);
  const { can } = usePermissions();
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
      WO_FILTER_STATUSES.map((code) => ({
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
  const [lifecycleFilter, setLifecycleFilter] = useState<string | null>(null);
  const [printError, setPrintError] = useState<string | null>(null);
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

  const handleLifecycleFilter = useCallback(
    (val: string | null) => {
      setLifecycleFilter(val);
      setFilter({ lifecycle_states: val ? [val] : null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const LIFECYCLE_OPTIONS = useMemo(
    () => WO_LIFECYCLE_STATES.map((code) => ({ value: code, label: t(`lifecycle.${code}`) })),
    [t],
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
    setLifecycleFilter(null);
    setTypeFilter(null);
    setPriorityFilter(null);
    setFilter({
      search: null,
      status_codes: null,
      lifecycle_states: null,
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
        id: "lifecycle",
        kind: "select",
        label: t("list.filters.lifecycle"),
        options: LIFECYCLE_OPTIONS,
        value: lifecycleFilter,
        onChange: handleLifecycleFilter,
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
      LIFECYCLE_OPTIONS,
      TYPE_OPTIONS,
      PRIORITY_OPTIONS,
      statusFilter,
      lifecycleFilter,
      typeFilter,
      priorityFilter,
      handleStatusFilter,
      handleLifecycleFilter,
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
        id: "equipment",
        accessorFn: (wo) => wo.asset_label ?? "",
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
        id: "status",
        accessorFn: (wo) => wo.status_code ?? "draft",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.status_code ?? "draft";
          const lc = row.original.lifecycle_state;
          return (
            <div className="flex flex-col items-start gap-0.5">
              <StatusChip
                label={t(`status.${statusToI18nKey(s)}`)}
                tone={toneOf(WO_STATUS_TONE, s)}
                dot
              />
              {isWoLifecycleState(lc) ? (
                <span className="text-[10px] text-text-muted">{t(`lifecycle.${lc}`)}</span>
              ) : null}
            </div>
          );
        },
      },
      {
        id: "responsible",
        accessorFn: (wo) => wo.responsible_display_name ?? wo.responsible_username ?? "",
        header: t("list.columns.assignedTo"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.responsible_display_name ?? row.original.responsible_username ?? "—"}
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
            <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
              <WoContextMenu
                wo={wo}
                onViewDetail={(item) => void openWo(item.id)}
                onEdit={(item) => openCreateForm(item)}
                onPrint={(item) => {
                  setPrintError(null);
                  printWoFiche(item, t, i18n.resolvedLanguage || i18n.language || "fr").catch(
                    (err: unknown) => setPrintError(toErrorMessage(err)),
                  );
                }}
                onCancel={(item) => setCancelTarget(item)}
              >
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-10 w-10 p-0"
                  aria-label={t("contextMenu.actionsFor", { code: wo.code })}
                >
                  <MoreHorizontal className="h-4 w-4" aria-hidden />
                </Button>
              </WoContextMenu>
            </div>
          );
        },
      },
    ],
    [t, i18n.language, i18n.resolvedLanguage, openWo, openCreateForm],
  );

  const viewOptions = useMemo<ViewOption<WoViewMode>[]>(
    () => [
      { value: "list", label: t("page.viewList"), icon: List },
      { value: "kanban", label: t("page.viewKanban"), icon: Columns3 },
      { value: "calendar", label: t("page.viewCalendar"), icon: CalendarDays },
      { value: "dashboard", label: t("page.viewDashboard"), icon: BarChart3 },
      {
        value: "integrity",
        label: t("page.viewIntegrity"),
        icon: Shield,
        hidden: !can(P.INTEGRITY_REPAIR),
      },
    ],
    [t, can],
  );

  // A view hidden by permissions (e.g. remembered from another account) falls back to the list.
  const activeView: WoViewMode =
    viewOptions.find((o) => o.value === view && !o.hidden)?.value ?? "list";

  const activeFilterCount = [statusFilter, lifecycleFilter, typeFilter, priorityFilter].filter(
    Boolean,
  ).length;

  const showListError = listError != null && items.length === 0;
  const recordView = activeView === "list" || activeView === "kanban" || activeView === "calendar";

  return (
    <ModulePageShell
      icon={Wrench}
      title={t("page.title")}
      count={total}
      bodyClassName={null}
      actions={
        <>
          <PermissionGate permission={P.OT_CREATE}>
            <Button onClick={() => openCreateForm()} className={mfLayout.primaryAction}>
              <Plus className="h-4 w-4" aria-hidden />
              {t("action.create")}
            </Button>
          </PermissionGate>
          <ViewToggle value={activeView} options={viewOptions} onChange={switchView} />
          <FilterToggleButton
            open={showFilters}
            onToggle={toggleFilters}
            activeCount={activeFilterCount}
          />
          <RefreshButton onRefresh={() => void loadWos()} loading={loading} />
        </>
      }
      filterBar={
        <>
          {printError ? (
            <p
              role="alert"
              className="mx-6 mt-2 rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
            >
              {t("print.failed", { message: printError })}
            </p>
          ) : null}
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
          {/* Requests awaiting conversion (ot.edit permission) */}
          <PermissionGate permission={P.OT_EDIT}>
            <WoDiManagementPanel />
          </PermissionGate>
        </>
      }
    >
      {recordView && showListError ? (
        <div className="p-4">
          <ErrorState error={listError} onRetry={() => void loadWos()} retrying={loading} />
        </div>
      ) : null}
      {recordView && !showListError && items.length < total ? (
        <div className="px-4 pt-3">
          <TruncationNotice
            shown={items.length}
            total={total}
            onLoadMore={() => void loadMoreWos()}
            loadingMore={loadingMore}
          />
        </div>
      ) : null}
      {activeView === "integrity" && <WoIntegrityWorkbench />}
      {activeView === "list" && !showListError && (
        <div className="p-4">
          <DataTable
            columns={columns}
            data={items}
            pageSize={20}
            isLoading={loading}
            skeletonRows={8}
            onRowClick={(row) => void openWo(row.id)}
          />
        </div>
      )}
      {activeView === "kanban" && !showListError && (
        <WoKanbanView items={items} onCardClick={(wo) => void openWo(wo.id)} />
      )}
      {activeView === "calendar" && !showListError && (
        <WoCalendarView items={items} onSelect={(wo) => void openWo(wo.id)} />
      )}
      {activeView === "dashboard" && <WoDashboardView />}

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
    </ModulePageShell>
  );
}

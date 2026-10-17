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
import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
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
import { useWoTypeLabels } from "@/components/wo/use-wo-type-labels";
import { mfLayout } from "@/design-system/tokens";
import { WO_STATUS_TONE, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import {
  LIST_PARAM,
  type ListScope,
  type WorkMix,
  idParam,
  mixParam,
  natureInMix,
  scopeParam,
  textParam,
  withoutParams,
} from "@/lib/list-links";
import { getAssetByIdSilent } from "@/services/asset-service";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { formatDate } from "@/utils/format-date";
import { WO_LIFECYCLE_STATES, isWoLifecycleState } from "@/utils/wo-lifecycle";
import { statusToI18nKey } from "@/utils/wo-status";
import type { UrgencyCode, WorkOrder } from "@shared/ipc-types";
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

/** "Open" work orders of the dashboards: every lifecycle state except closed and cancelled. */

// ── Component ───────────────────────────────────────────────────────────────

export function WorkOrdersPage() {
  const { t, i18n } = useTranslation("ot");
  const items = useWoStore((s) => s.items);
  const listFilter = useWoStore((s) => s.filter);
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
  const { levels: urgencyLevels, labelOf: urgencyLabelOf } = useUrgencyScale();
  const { typeLabel } = useWoTypeLabels();

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
          label: typeLabel(type.code, type.label),
        })),
    [woTypes, typeLabel],
  );

  const PRIORITY_OPTIONS = useMemo(
    () =>
      urgencyLevels.map((level) => ({
        value: level.code,
        label: urgencyLabelOf(level.code),
      })),
    [urgencyLevels, urgencyLabelOf],
  );

  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [lifecycleFilter, setLifecycleFilter] = useState<string | null>(null);
  const [printError, setPrintError] = useState<string | null>(null);
  const [typeFilter, setTypeFilter] = useState<string | null>(null);
  const [priorityFilter, setPriorityFilter] = useState<string | null>(null);
  /** From a dashboard link: open work orders only (shown as a chip, clearable). */
  const [scopeFilter, setScopeFilter] = useState<ListScope | null>(null);
  /** From a work-mix link (SMRP / home): every type of that nature group (chip, clearable). */
  const [mixFilter, setMixFilter] = useState<WorkMix | null>(null);
  /** From an equipment link: one asset (chip shows its code). */
  const [equipmentFilter, setEquipmentFilter] = useState<{ id: number; label: string } | null>(
    null,
  );

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
      setFilter({ urgency_code: val ? (val as UrgencyCode) : null });
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
    setScopeFilter(null);
    setEquipmentFilter(null);
    setMixFilter(null);
    setFilter({
      search: null,
      status_codes: null,
      lifecycle_states: null,
      type_codes: null,
      urgency_code: null,
      equipment_id: null,
      open_only: null,
      not_closed_only: null,
      overdue_only: null,
    });
    void loadWos();
  }, [setFilter, loadWos]);

  const handleScopeFilter = useCallback(
    (val: string | null) => {
      const scope = scopeParam(val);
      setScopeFilter(scope);
      setFilter({
        open_only: scope === "open" || null,
        not_closed_only: scope === "not_closed" || null,
        overdue_only: scope === "overdue" || null,
      });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handleMixFilter = useCallback(
    (val: string | null) => {
      if (val) return; // only clearing is offered: the mix comes from the link
      setMixFilter(null);
      setFilter({ type_codes: null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  const handleEquipmentFilter = useCallback(
    (val: string | null) => {
      if (val) return; // only clearing is offered: the asset comes from the link
      setEquipmentFilter(null);
      setFilter({ equipment_id: null });
      void loadWos();
    },
    [setFilter, loadWos],
  );

  // A link may carry list filters (?scope=open, ?status=, ?urgency=, ?equipment=): they become the
  // page's filters — visible as chips the user can clear — then leave the address, so the chips
  // are the only source of truth. Without such a link the page keeps its filters as before.
  useEffect(() => {
    const scope = scopeParam(searchParams.get(LIST_PARAM.scope));
    const status = textParam(searchParams.get(LIST_PARAM.status));
    const urgency = textParam(searchParams.get(LIST_PARAM.urgency));
    const equipmentId = idParam(searchParams.get(LIST_PARAM.equipment));
    const type = textParam(searchParams.get(LIST_PARAM.type));
    const mix = mixParam(searchParams.get(LIST_PARAM.mix));
    if (!scope && !status && !urgency && equipmentId == null && !type && !mix) return;
    // The mix is resolved through the type catalogue: wait for it before applying the link.
    if (mix && woTypes.length === 0) return;
    const mixCodes = mix
      ? woTypes.filter((wt) => natureInMix(wt.nature, mix)).map((wt) => wt.code)
      : null;
    setSearchInput("");
    setStatusFilter(status);
    setLifecycleFilter(null);
    setTypeFilter(mix ? null : type);
    setMixFilter(mix);
    setPriorityFilter(urgency);
    setScopeFilter(scope);
    setEquipmentFilter(equipmentId != null ? { id: equipmentId, label: `#${equipmentId}` } : null);
    setFilter({
      search: null,
      status_codes: status ? [status] : null,
      lifecycle_states: null,
      // The list applies the same rule as the count the link came from.
      open_only: scope === "open" || null,
      not_closed_only: scope === "not_closed" || null,
      overdue_only: scope === "overdue" || null,
      // A mix with no matching type matches no work order (never "all types").
      type_codes: mixCodes ? (mixCodes.length > 0 ? mixCodes : ["__none__"]) : type ? [type] : null,
      urgency_code: urgency ? (urgency as UrgencyCode) : null,
      equipment_id: equipmentId,
    });
    void loadWos();
    if (equipmentId != null) {
      void getAssetByIdSilent(equipmentId).then((asset) => {
        if (asset) {
          setEquipmentFilter({
            id: equipmentId,
            label: `${asset.asset_code} — ${asset.asset_name}`,
          });
        }
      });
    }
    setSearchParams(
      withoutParams(
        searchParams,
        LIST_PARAM.scope,
        LIST_PARAM.status,
        LIST_PARAM.urgency,
        LIST_PARAM.equipment,
        LIST_PARAM.type,
        LIST_PARAM.mix,
      ),
      { replace: true },
    );
  }, [searchParams, setSearchParams, setFilter, loadWos, woTypes]);

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
      ...(scopeFilter
        ? [
            {
              id: "scope",
              kind: "select" as const,
              label: t("list.filters.scope"),
              options: [
                {
                  value: scopeFilter,
                  label:
                    scopeFilter === "overdue"
                      ? t("list.filters.scopeOverdue")
                      : scopeFilter === "not_closed"
                        ? t("list.filters.scopeNotClosed")
                        : t("list.filters.scopeOpen"),
                },
              ],
              value: scopeFilter,
              onChange: handleScopeFilter,
            },
          ]
        : []),
      ...(mixFilter
        ? [
            {
              id: "mix",
              kind: "select" as const,
              label: t("list.filters.mix"),
              options: [{ value: mixFilter, label: t(`list.filters.mixValue.${mixFilter}`) }],
              value: mixFilter,
              onChange: handleMixFilter,
            },
          ]
        : []),
      ...(equipmentFilter
        ? [
            {
              id: "equipment",
              kind: "select" as const,
              label: t("list.filters.equipment"),
              options: [{ value: String(equipmentFilter.id), label: equipmentFilter.label }],
              value: String(equipmentFilter.id),
              onChange: handleEquipmentFilter,
            },
          ]
        : []),
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
      scopeFilter,
      equipmentFilter,
      mixFilter,
      handleScopeFilter,
      handleEquipmentFilter,
      handleMixFilter,
    ],
  );

  // ── Load on mount ─────────────────────────────────────────────────────

  useEffect(() => {
    // A link carrying list filters loads the list itself (with those filters), once.
    const p = new URLSearchParams(window.location.search);
    const linked = [
      LIST_PARAM.scope,
      LIST_PARAM.status,
      LIST_PARAM.urgency,
      LIST_PARAM.equipment,
      LIST_PARAM.type,
      LIST_PARAM.mix,
    ];
    if (linked.some((k) => p.has(k))) return;
    void loadWos();
  }, [loadWos]);

  useEffect(() => {
    void loadWoTypes();
  }, [loadWoTypes]);

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
        cell: ({ row }) => (
          <span className="text-xs">
            {typeLabel(row.original.type_code, row.original.type_label) || "—"}
          </span>
        ),
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
                    (err: unknown) => setPrintError(describeAppError(err).title),
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
    [t, i18n.language, i18n.resolvedLanguage, openWo, openCreateForm, typeLabel],
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
        <WoKanbanView
          filter={listFilter}
          reloadSignal={items}
          onCardClick={(wo) => void openWo(wo.id)}
        />
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

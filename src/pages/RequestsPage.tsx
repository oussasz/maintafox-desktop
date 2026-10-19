/**
 * RequestsPage.tsx
 *
 * Intervention requests (DI) workspace. A bar of work queues (to screen, to review, waiting for
 * information, to approve, approved without a work order, deferral due, SLA at risk, my requests)
 * sits above the list, kanban and calendar views; the dashboard view holds the KPIs.
 * Detail: floating dialog (DiDetailDialog) over the view.
 */

import type { ColumnDef } from "@tanstack/react-table";
import {
  BarChart3,
  CalendarDays,
  ClipboardCheck,
  ClipboardList,
  Columns3,
  List,
  MoreHorizontal,
  Plus,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { UrgencyBadge } from "@/components/common/urgency/UrgencyBadge";
import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { DataTable } from "@/components/data/DataTable";
import { DiApprovalDialog } from "@/components/di/DiApprovalDialog";
import { DiArchivePanel } from "@/components/di/DiArchivePanel";
import { DiCalendarView } from "@/components/di/DiCalendarView";
import { DiCloseDialog } from "@/components/di/DiCloseDialog";
import { DiContextMenu } from "@/components/di/DiContextMenu";
import { DiDashboardView } from "@/components/di/DiDashboardView";
import { DiDetailDialog } from "@/components/di/DiDetailDialog";
import { DiFormDialog } from "@/components/di/DiFormDialog";
import { DiKanbanBoard } from "@/components/di/DiKanbanBoard";
import { DiNextStepLabel } from "@/components/di/DiNextStepLabel";
import { printDiFiche } from "@/components/di/DiPrintFiche";
import { DiQueueBar, type DiQueueSelection, useVisibleDiQueues } from "@/components/di/DiQueueBar";
import { DiReturnDialog } from "@/components/di/DiReturnDialog";
import { DiHiddenRequestsNotice, DiNoSiteBadge } from "@/components/di/DiSiteScope";
import { DiSlaRulesPanel } from "@/components/di/DiSlaRulesPanel";
import { DiSlaStatusBadge } from "@/components/di/DiSlaStatusBadge";
import { DI_DISPOSITION_FILTER_CODES } from "@/components/di/disposition-meta";
import { diStatusToI18nKey } from "@/components/di/status-meta";
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
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { usePlantWideAccess } from "@/components/wo/WoSiteScope";
import { mfLayout } from "@/design-system/tokens";
import { DI_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useAccessAtNodes } from "@/hooks/use-access-at";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { formatPersonLabel } from "@/lib/display";
import {
  LIST_PARAM,
  idParam,
  scopeParam,
  textParam,
  waitingDaysParam,
  withoutParams,
} from "@/lib/list-links";
import { getAssetByIdSilent } from "@/services/asset-service";
import { triageSubmittedDi } from "@/services/di-service";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { DiQueue, InterventionRequest } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ViewMode = "list" | "kanban" | "calendar" | "dashboard";

const QUEUE_PARAM = "queue";

/** Older links: `?triage=1` (entry queue) and `?review=1` (review queue). */
function queueFromParams(params: URLSearchParams): DiQueueSelection | null {
  const raw = params.get(QUEUE_PARAM);
  if (raw) return raw as DiQueueSelection;
  if (params.get("triage") === "1") return "to_screen";
  if (params.get("review") === "1") return "to_review";
  return null;
}

function readStoredView(): ViewMode {
  try {
    const v = localStorage.getItem("di-view-mode");
    if (v === "list" || v === "kanban" || v === "calendar" || v === "dashboard") return v;
  } catch {
    // Storage unavailable: default view.
  }
  return "list";
}

function storeView(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable: the choice lasts for this session only.
  }
}

// ── Component ───────────────────────────────────────────────────────────────

export function RequestsPage() {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [searchParams, setSearchParams] = useSearchParams();
  const items = useDiStore((s) => s.items);
  const listFilter = useDiStore((s) => s.filter);
  const total = useDiStore((s) => s.total);
  const hiddenOutsideScope = useDiStore((s) => s.hiddenOutsideScope);
  const hiddenWithoutSite = useDiStore((s) => s.hiddenWithoutSite);
  const loading = useDiStore((s) => s.loading);
  const loadingMore = useDiStore((s) => s.loadingMore);
  const listError = useDiStore((s) => s.listError);
  const activeDi = useDiStore((s) => s.activeDi);
  const queueCounts = useDiStore((s) => s.queueCounts);
  const loadDis = useDiStore((s) => s.loadDis);
  const loadMoreDis = useDiStore((s) => s.loadMoreDis);
  const loadQueueCounts = useDiStore((s) => s.loadQueueCounts);
  const openDi = useDiStore((s) => s.openDi);
  const closeDi = useDiStore((s) => s.closeDi);
  const openCreateForm = useDiStore((s) => s.openCreateForm);
  const setFilter = useDiStore((s) => s.setFilter);
  const openApproval = useDiReviewStore((s) => s.openApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const openReturn = useDiReviewStore((s) => s.openReturn);

  const { can, isLoading: permissionsLoading } = usePermissions();
  const visibleQueues = useVisibleDiQueues();

  const [view, setView] = useState<ViewMode>(readStoredView);
  const [showFilters, setShowFilters] = useState(() => {
    try {
      return localStorage.getItem("di-show-filters") !== "0";
    } catch {
      return true;
    }
  });
  const [searchInput, setSearchInput] = useState("");
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [priorityFilter, setPriorityFilter] = useState<string | null>(null);
  const [dispositionFilter, setDispositionFilter] = useState<string | null>(null);
  /** From a dashboard link: requests still being handled (chip, clearable). */
  const [scopeFilter, setScopeFilter] = useState<"open" | null>(null);
  /** From the overdue figure: open requests waiting at least this many days (chip, clearable). */
  const [waitingFilter, setWaitingFilter] = useState<number | null>(null);
  /** From an equipment link: one asset (chip shows its code). */
  const [equipmentFilter, setEquipmentFilter] = useState<{ id: number; label: string } | null>(
    null,
  );
  const [archiveRefreshKey, setArchiveRefreshKey] = useState(0);
  const [slaOpen, setSlaOpen] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [bulkBusy, setBulkBusy] = useState(false);
  const [bulkMessage, setBulkMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  // A requester without a review role lands on their own requests.
  const defaultQueue: DiQueueSelection =
    visibleQueues.length <= 2 && visibleQueues.includes("mine") ? "mine" : "all";
  const requestedQueue = queueFromParams(searchParams);
  const queue: DiQueueSelection =
    requestedQueue && visibleQueues.includes(requestedQueue) ? requestedQueue : defaultQueue;

  // The queue drives the list filter; status / urgency / disposition filters narrow it further.
  // The default queue depends on the user's role: wait for the permissions so the page does not
  // open on one queue and jump to another.
  useEffect(() => {
    if (permissionsLoading) return;
    setFilter({ queue: queue === "all" ? null : (queue as DiQueue), offset: 0 });
    setSelected(new Set());
    setBulkMessage(null);
    void loadDis();
  }, [queue, permissionsLoading, setFilter, loadDis]);

  useEffect(() => {
    const raw = searchParams.get("openDi");
    if (!raw) return;
    const id = Number.parseInt(raw, 10);
    if (!Number.isNaN(id)) void openDi(id);
    setSearchParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        p.delete("openDi");
        return p;
      },
      { replace: true },
    );
  }, [searchParams, openDi, setSearchParams]);

  // Queue sizes follow every list reload (any decision reloads the list).
  useEffect(() => {
    if (loading) return;
    void loadQueueCounts();
    setArchiveRefreshKey((k) => k + 1);
  }, [loading, loadQueueCounts]);

  useEffect(() => {
    const onRefresh = () => {
      void loadDis();
    };
    window.addEventListener("mf:di-triage-refresh", onRefresh);
    return () => window.removeEventListener("mf:di-triage-refresh", onRefresh);
  }, [loadDis]);

  const switchQueue = useCallback(
    (next: DiQueueSelection) => {
      setSearchParams(
        (prev) => {
          const p = new URLSearchParams(prev);
          p.delete("triage");
          p.delete("review");
          if (next === defaultQueue) p.delete(QUEUE_PARAM);
          else p.set(QUEUE_PARAM, next);
          return p;
        },
        { replace: true },
      );
      if (view === "dashboard") {
        setView("list");
        storeView("di-view-mode", "list");
      }
    },
    [defaultQueue, setSearchParams, view],
  );

  const switchView = useCallback((v: ViewMode) => {
    setView(v);
    storeView("di-view-mode", v);
  }, []);

  const STATUS_OPTIONS = useMemo(
    () =>
      Object.keys(DI_STATUS_TONE).map((code) => ({
        value: code,
        label: t(`status.${diStatusToI18nKey(code)}` as const),
      })),
    [t],
  );

  const { levels: urgencyLevels, labelOf: urgencyLabel } = useUrgencyScale();
  const PRIORITY_OPTIONS = useMemo(
    () => urgencyLevels.map((level) => ({ value: level.code, label: urgencyLabel(level.code) })),
    [urgencyLevels, urgencyLabel],
  );

  const DISPOSITION_OPTIONS = useMemo(
    () =>
      DI_DISPOSITION_FILTER_CODES.map((code) => ({
        value: code,
        label: t(`disposition.${code}` as const),
      })),
    [t],
  );

  const onSearchChange = useCallback(
    (val: string) => {
      setFilter({ search: val.trim() || null, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleStatusFilter = useCallback(
    (val: string | null) => {
      setStatusFilter(val);
      setFilter({ status: val == null ? null : [val], offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handlePriorityFilter = useCallback(
    (val: string | null) => {
      setPriorityFilter(val);
      setFilter({ urgency: val, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleDispositionFilter = useCallback(
    (val: string | null) => {
      setDispositionFilter(val);
      setFilter({ disposition_code: val, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setStatusFilter(null);
    setPriorityFilter(null);
    setDispositionFilter(null);
    setScopeFilter(null);
    setWaitingFilter(null);
    setEquipmentFilter(null);
    setFilter({
      search: null,
      status: null,
      urgency: null,
      disposition_code: null,
      equipment_id: null,
      open_only: null,
      waiting_days: null,
      offset: 0,
    });
    void loadDis();
  }, [loadDis, setFilter]);

  const handleScopeFilter = useCallback(
    (val: string | null) => {
      const scope = val === "open" ? "open" : null;
      setScopeFilter(scope);
      setFilter({ open_only: scope === "open" || null, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleWaitingFilter = useCallback(
    (val: string | null) => {
      if (val) return; // only clearing is offered: the waiting time comes from the link
      setWaitingFilter(null);
      setFilter({ waiting_days: null, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleEquipmentFilter = useCallback(
    (val: string | null) => {
      if (val) return; // only clearing is offered: the asset comes from the link
      setEquipmentFilter(null);
      setFilter({ equipment_id: null, offset: 0 });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  // A link may carry list filters (?scope=open, ?status=, ?urgency=, ?equipment=) next to the
  // queue: they become the page's filters, shown as chips the user can clear, then leave the
  // address (the queue stays: the queue bar shows it).
  useEffect(() => {
    if (permissionsLoading) return;
    const scope = scopeParam(searchParams.get(LIST_PARAM.scope)) === "open" ? "open" : null;
    const waitingDays = waitingDaysParam(searchParams.get(LIST_PARAM.waiting));
    const status = textParam(searchParams.get(LIST_PARAM.status));
    const urgency = textParam(searchParams.get(LIST_PARAM.urgency));
    const equipmentId = idParam(searchParams.get(LIST_PARAM.equipment));
    if (!scope && waitingDays == null && !status && !urgency && equipmentId == null) return;
    setSearchInput("");
    setStatusFilter(status);
    setPriorityFilter(urgency);
    setDispositionFilter(null);
    setScopeFilter(scope);
    setWaitingFilter(waitingDays);
    setEquipmentFilter(equipmentId != null ? { id: equipmentId, label: `#${equipmentId}` } : null);
    setFilter({
      search: null,
      status: status ? [status] : null,
      // The list applies the same rules as the counts the link came from.
      open_only: scope === "open" || null,
      waiting_days: waitingDays,
      urgency,
      disposition_code: null,
      equipment_id: equipmentId,
      offset: 0,
    });
    void loadDis();
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
        LIST_PARAM.waiting,
        LIST_PARAM.status,
        LIST_PARAM.urgency,
        LIST_PARAM.equipment,
      ),
      { replace: true },
    );
  }, [permissionsLoading, searchParams, setSearchParams, setFilter, loadDis]);

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
        id: "priority",
        kind: "select",
        label: t("list.filters.priority"),
        options: PRIORITY_OPTIONS,
        value: priorityFilter,
        onChange: handlePriorityFilter,
      },
      {
        id: "disposition",
        kind: "select",
        label: t("list.filters.disposition"),
        options: DISPOSITION_OPTIONS,
        value: dispositionFilter,
        onChange: handleDispositionFilter,
      },
      ...(scopeFilter
        ? [
            {
              id: "scope",
              kind: "select" as const,
              label: t("list.filters.scope"),
              options: [{ value: "open", label: t("list.filters.scopeOpen") }],
              value: scopeFilter,
              onChange: handleScopeFilter,
            },
          ]
        : []),
      ...(waitingFilter != null
        ? [
            {
              id: "waiting",
              kind: "select" as const,
              label: t("list.filters.waiting"),
              options: [
                {
                  value: String(waitingFilter),
                  label: t("list.filters.waitingValue", { count: waitingFilter }),
                },
              ],
              value: String(waitingFilter),
              onChange: handleWaitingFilter,
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
      PRIORITY_OPTIONS,
      DISPOSITION_OPTIONS,
      statusFilter,
      priorityFilter,
      dispositionFilter,
      handleStatusFilter,
      handlePriorityFilter,
      handleDispositionFilter,
      scopeFilter,
      waitingFilter,
      equipmentFilter,
      handleScopeFilter,
      handleWaitingFilter,
      handleEquipmentFilter,
    ],
  );

  const toggleFilters = useCallback(() => {
    setShowFilters((prev) => {
      const next = !prev;
      storeView("di-show-filters", next ? "1" : "0");
      return next;
    });
  }, []);

  // ── Bulk admission to review (the one safe bulk decision: nothing is lost or closed) ──

  const canBulkTriage = queue === "to_screen" && (can(P.DI_SCREEN) || can(P.DI_REVIEW));
  const triageable = useMemo(
    () => items.filter((di) => di.status === "submitted").map((di) => di.id),
    [items],
  );

  // The backend checks di.screen / di.review at the site of EACH request: the button is enabled
  // only when the user holds one of them at every selected request's site (no site: plant-wide).
  const triageNodes = useMemo(
    () =>
      items
        .filter((di) => selected.has(di.id) && di.status === "submitted")
        .map((di) => (di.org_node_id != null && di.org_node_id > 0 ? di.org_node_id : 0)),
    [items, selected],
  );
  const triageAccess = useAccessAtNodes(triageNodes);
  const triageAllowed = triageAccess.canAllAny([P.DI_SCREEN, P.DI_REVIEW]);
  // SLA rules are plant-wide reference settings: the backend checks di.admin plant-wide.
  const plantWide = usePlantWideAccess();
  const canManageSla = plantWide.can(P.DI_ADMIN);

  const toggleRow = useCallback((id: number, on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  const runBulkTriage = useCallback(async () => {
    const targets = items.filter((di) => selected.has(di.id) && di.status === "submitted");
    if (targets.length === 0) return;
    setBulkBusy(true);
    setBulkMessage(null);
    let failed = 0;
    for (const di of targets) {
      try {
        await triageSubmittedDi({ di_id: di.id, expected_row_version: di.row_version });
      } catch {
        failed += 1;
      }
    }
    setBulkBusy(false);
    setSelected(new Set());
    setBulkMessage(
      t("bulk.triageResult", { done: targets.length - failed, failed, total: targets.length }),
    );
    void loadDis();
  }, [items, selected, loadDis, t]);

  const triageOne = useCallback(
    async (di: InterventionRequest) => {
      setActionError(null);
      try {
        await triageSubmittedDi({ di_id: di.id, expected_row_version: di.row_version });
        void loadDis();
      } catch (err) {
        setActionError(describeAppError(err).title);
      }
    },
    [loadDis],
  );

  const printOne = useCallback((di: InterventionRequest) => {
    setActionError(null);
    printDiFiche(di).catch((err: unknown) => setActionError(describeAppError(err).title));
  }, []);

  // ── Columns ─────────────────────────────────────────────────────────────

  const columns: ColumnDef<InterventionRequest>[] = useMemo(() => {
    const cols: ColumnDef<InterventionRequest>[] = [];
    if (canBulkTriage) {
      cols.push({
        id: "select",
        header: () => (
          <Checkbox
            aria-label={t("bulk.selectAll")}
            checked={triageable.length > 0 && triageable.every((id) => selected.has(id))}
            onCheckedChange={(v) => setSelected(v === true ? new Set(triageable) : new Set())}
          />
        ),
        cell: ({ row }) => (
          <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
            <Checkbox
              aria-label={t("bulk.selectOne", { code: row.original.code })}
              checked={selected.has(row.original.id)}
              disabled={row.original.status !== "submitted"}
              onCheckedChange={(v) => toggleRow(row.original.id, v === true)}
            />
          </div>
        ),
      });
    }
    cols.push(
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => (
          <span className="font-mono text-xs inline-flex items-center gap-1.5 flex-wrap">
            {row.original.code}
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.converted_to_wo_code}
              entityId={row.original.converted_to_wo_id}
              title={row.original.converted_to_wo_title}
              className="text-[10px] px-1.5 py-0"
            />
            <DiNoSiteBadge di={row.original} />
          </span>
        ),
      },
      {
        accessorKey: "title",
        header: t("list.columns.subject"),
        cell: ({ row }) => (
          <span className="max-w-[220px] truncate block" title={row.original.title}>
            {row.original.title}
          </span>
        ),
      },
      {
        id: "equipment",
        accessorFn: (di) => di.asset_label ?? di.asset_code ?? "",
        header: t("list.columns.equipment"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.asset_label ?? row.original.asset_code ?? "—"}
          </span>
        ),
      },
      {
        accessorKey: "status",
        header: t("list.columns.status"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`status.${diStatusToI18nKey(row.original.status)}` as const)}
            tone={toneOf(DI_STATUS_TONE, row.original.status)}
            dot
          />
        ),
      },
      {
        id: "urgency",
        accessorFn: (di) => di.validated_urgency ?? di.reported_urgency,
        header: t("list.columns.priority"),
        cell: ({ row }) => (
          <UrgencyBadge code={row.original.validated_urgency ?? row.original.reported_urgency} />
        ),
      },
      {
        id: "sla",
        accessorFn: (di) => di.sla_state ?? "",
        header: t("list.columns.sla"),
        cell: ({ row }) =>
          row.original.sla_state && row.original.sla_state !== "completed" ? (
            <DiSlaStatusBadge status={row.original.sla_state} requestStatus={row.original.status} />
          ) : (
            <span className="text-xs text-text-muted">—</span>
          ),
      },
      {
        id: "nextStep",
        header: t("list.columns.nextStep"),
        cell: ({ row }) => <DiNextStepLabel di={row.original} />,
      },
      {
        id: "requester",
        accessorFn: (di) => di.submitter_display_name ?? "",
        header: t("list.columns.reportedBy"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {formatPersonLabel(row.original.submitter_display_name)}
          </span>
        ),
      },
      {
        accessorKey: "submitted_at",
        header: t("list.columns.reportedAt"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {formatDiDate(row.original.submitted_at, dateLocale, {
              day: "2-digit",
              month: "2-digit",
              year: "numeric",
            })}
          </span>
        ),
      },
      {
        id: "actions",
        header: "",
        cell: ({ row }) => (
          <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
            <DiContextMenu
              di={row.original}
              onViewDetail={(di) => void openDi(di.id)}
              onEdit={(di) => openCreateForm(di)}
              onTriage={(di) => void triageOne(di)}
              onApprove={openApproval}
              onReturn={openReturn}
              onClose={openRejection}
              onPrint={printOne}
            >
              <Button
                variant="ghost"
                size="sm"
                className="h-10 w-10 p-0"
                aria-label={t("contextMenu.actionsFor", { code: row.original.code })}
              >
                <MoreHorizontal className="h-4 w-4" aria-hidden />
              </Button>
            </DiContextMenu>
          </div>
        ),
      },
    );
    return cols;
  }, [
    canBulkTriage,
    triageable,
    selected,
    toggleRow,
    t,
    dateLocale,
    openDi,
    openCreateForm,
    triageOne,
    openApproval,
    openReturn,
    openRejection,
    printOne,
  ]);

  const viewOptions: ViewOption<ViewMode>[] = [
    { value: "list", label: t("page.viewList"), icon: List },
    { value: "kanban", label: t("page.viewKanban"), icon: Columns3 },
    { value: "calendar", label: t("page.viewCalendar"), icon: CalendarDays },
    { value: "dashboard", label: t("page.viewDashboard"), icon: BarChart3 },
  ];
  const activeFilterCount = [statusFilter, priorityFilter, dispositionFilter].filter(
    Boolean,
  ).length;
  const showListError = listError != null && items.length === 0;
  const recordView = view === "list" || view === "kanban" || view === "calendar";
  const showEmpty = recordView && !loading && !showListError && items.length === 0;
  const selectedCount = [...selected].filter((id) => triageable.includes(id)).length;

  return (
    <ModulePageShell
      icon={ClipboardList}
      title={t("page.title")}
      count={total}
      bodyClassName={null}
      actions={
        <>
          <PermissionGate anyOf={[P.DI_CREATE, P.DI_CREATE_OWN]}>
            <Button onClick={() => openCreateForm()} className={mfLayout.primaryAction}>
              <Plus className="h-4 w-4" aria-hidden />
              {t("action.create")}
            </Button>
          </PermissionGate>
          <ViewToggle value={view} options={viewOptions} onChange={switchView} />
          <FilterToggleButton
            open={showFilters}
            onToggle={toggleFilters}
            activeCount={activeFilterCount}
          />
          {canManageSla && (
            <Button variant="outline" className="h-11" onClick={() => setSlaOpen(true)}>
              {t("sla.title")}
            </Button>
          )}
          <RefreshButton onRefresh={() => void loadDis()} loading={loading} />
        </>
      }
      filterBar={
        <>
          {permissionsLoading ? null : (
            <DiQueueBar value={queue} counts={queueCounts} onChange={switchQueue} />
          )}
          {showFilters && recordView && (
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
        </>
      }
    >
      {recordView && showListError ? (
        <div className="p-4">
          <ErrorState error={listError} onRetry={() => void loadDis()} retrying={loading} />
        </div>
      ) : null}
      {actionError ? (
        <div className="px-4 pt-3">
          <p role="alert" className="text-sm text-tone-danger-fg">
            {actionError}
          </p>
        </div>
      ) : null}
      {recordView && canBulkTriage && (selectedCount > 0 || bulkMessage) ? (
        <div className="mx-4 mt-3 flex flex-wrap items-center gap-3 rounded-lg border border-surface-border bg-surface-1 px-4 py-2.5">
          {selectedCount > 0 ? (
            <>
              <span className="text-sm text-text-primary">
                {t("bulk.selected", { count: selectedCount })}
              </span>
              <Button
                size="sm"
                className="gap-1.5"
                disabled={bulkBusy || !triageAllowed}
                title={triageAllowed ? undefined : t("bulk.outsideYourSites")}
                onClick={() => void runBulkTriage()}
              >
                <ClipboardCheck className="h-3.5 w-3.5" aria-hidden />
                {t("bulk.triage", { count: selectedCount })}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={bulkBusy}
                onClick={() => setSelected(new Set())}
              >
                {t("bulk.clear")}
              </Button>
              {!triageAllowed && !triageAccess.loading ? (
                <span role="status" className="text-xs text-tone-warning-fg">
                  {t("bulk.outsideYourSites")}
                </span>
              ) : null}
            </>
          ) : null}
          {bulkMessage ? (
            <span role="status" className="text-sm text-text-secondary">
              {bulkMessage}
            </span>
          ) : null}
        </div>
      ) : null}
      {recordView && !showListError ? (
        <DiHiddenRequestsNotice
          outsideScope={hiddenOutsideScope}
          withoutSite={hiddenWithoutSite}
          className="mx-4 mt-3"
        />
      ) : null}
      {recordView && !showListError && items.length < total ? (
        <div className="px-4 pt-3">
          <TruncationNotice
            shown={items.length}
            total={total}
            onLoadMore={() => void loadMoreDis()}
            loadingMore={loadingMore}
          />
        </div>
      ) : null}
      {showEmpty ? (
        <div className="p-4">
          <EmptyState
            title={t(`queues.empty.${queue}`)}
            description={activeFilterCount > 0 ? t("empty.listHint") : t("queues.emptyHint")}
          />
        </div>
      ) : null}
      {view === "list" && !showListError && !showEmpty && (
        <div className="p-4">
          <DataTable
            columns={columns}
            data={items}
            pageSize={20}
            isLoading={loading}
            skeletonRows={8}
            onRowClick={(row) => void openDi(row.id)}
          />
        </div>
      )}
      {view === "kanban" && !showListError && !showEmpty && (
        <DiKanbanBoard
          filter={listFilter}
          reloadSignal={items}
          onCardClick={(di) => void openDi(di.id)}
        />
      )}
      {view === "calendar" && !showListError && !showEmpty && (
        <DiCalendarView items={items} onChipClick={(di) => void openDi(di.id)} />
      )}
      {view === "dashboard" && <DiDashboardView onOpenQueue={switchQueue} />}

      {/* ── Archive section (collapsible) ─────────────────────────────── */}
      {recordView && queue === "all" ? (
        <DiArchivePanel refreshKey={archiveRefreshKey} onRowClick={(di) => void openDi(di.id)} />
      ) : null}

      {/* ── Floating detail dialog ───────────────────────────────────── */}
      <DiDetailDialog
        di={activeDi?.di ?? null}
        transitions={activeDi?.transitions ?? []}
        similar={activeDi?.similar ?? []}
        open={activeDi !== null}
        onClose={closeDi}
      />

      <DiFormDialog />

      {/* Review dialogs */}
      <DiApprovalDialog />
      <DiCloseDialog />
      <DiReturnDialog />

      {canManageSla && <DiSlaRulesPanel open={slaOpen} onClose={() => setSlaOpen(false)} />}
    </ModulePageShell>
  );
}

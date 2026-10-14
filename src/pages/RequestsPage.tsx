/**
 * RequestsPage.tsx
 *
 * Multi-view intervention-request (DI) workspace inspired by the web version.
 * Views: List (DataTable) and Kanban board, toggled via header buttons.
 * Detail: floating dialog (DiDetailDialog) — the underlying view stays fully
 * visible and usable when the dialog is closed.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { BarChart3, CalendarDays, ClipboardList, Columns3, List, Plus } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { DiApprovalDialog } from "@/components/di/DiApprovalDialog";
import { DiArchivePanel } from "@/components/di/DiArchivePanel";
import { DiCalendarView } from "@/components/di/DiCalendarView";
import { DiCloseDialog } from "@/components/di/DiCloseDialog";
import { DiDashboardView } from "@/components/di/DiDashboardView";
import { DiDetailDialog } from "@/components/di/DiDetailDialog";
import { DiFormDialog } from "@/components/di/DiFormDialog";
import { DiKanbanBoard } from "@/components/di/DiKanbanBoard";
import { DiReturnDialog } from "@/components/di/DiReturnDialog";
import { DiReviewPanel } from "@/components/di/DiReviewPanel";
import { DiSlaRulesPanel } from "@/components/di/DiSlaRulesPanel";
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
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { DI_STATUS_TONE, DI_URGENCY_TONE, toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ViewMode = "list" | "kanban" | "calendar" | "dashboard";

/** Same cohort as `loadReviewQueue` in `di-review-store` (pending triage + approval). */
const DI_REVIEW_QUEUE_STATUSES = [
  "in_review",
  "returned_for_clarification",
  "awaiting_approval",
] as const;

const STATUS_FILTER_REVIEW_QUEUE = "__review_queue__";
const STATUS_FILTER_TRIAGE_INBOX = "__triage_inbox__";

// ── Component ───────────────────────────────────────────────────────────────

export function RequestsPage() {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [searchParams, setSearchParams] = useSearchParams();
  const items = useDiStore((s) => s.items);
  const total = useDiStore((s) => s.total);
  const loading = useDiStore((s) => s.loading);
  const loadingMore = useDiStore((s) => s.loadingMore);
  const listError = useDiStore((s) => s.listError);
  const activeDi = useDiStore((s) => s.activeDi);
  const loadDis = useDiStore((s) => s.loadDis);
  const loadMoreDis = useDiStore((s) => s.loadMoreDis);
  const openDi = useDiStore((s) => s.openDi);
  const closeDi = useDiStore((s) => s.closeDi);
  const openCreateForm = useDiStore((s) => s.openCreateForm);
  const setFilter = useDiStore((s) => s.setFilter);

  const [view, setView] = useState<ViewMode>(
    () => (localStorage.getItem("di-view-mode") as ViewMode) || "kanban",
  );
  const [showFilters, setShowFilters] = useState(
    () => localStorage.getItem("di-show-filters") !== "0",
  );
  const [searchInput, setSearchInput] = useState("");
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [priorityFilter, setPriorityFilter] = useState<string | null>(null);
  const [dispositionFilter, setDispositionFilter] = useState<string | null>(null);
  const [archiveRefreshKey, setArchiveRefreshKey] = useState(0);

  const { can } = usePermissions();
  const [slaOpen, setSlaOpen] = useState(false);

  useLayoutEffect(() => {
    if (searchParams.get("review") !== "1") {
      return;
    }
    setView("list");
    localStorage.setItem("di-view-mode", "list");
    setStatusFilter(STATUS_FILTER_REVIEW_QUEUE);
    setFilter({
      status: [...DI_REVIEW_QUEUE_STATUSES],
      submitter_id: null,
      limit: 200,
      offset: 0,
    });
  }, [searchParams, setFilter, setView]);

  useLayoutEffect(() => {
    if (searchParams.get("triage") !== "1") {
      return;
    }
    setView("list");
    localStorage.setItem("di-view-mode", "list");
    setStatusFilter(STATUS_FILTER_TRIAGE_INBOX);
    setFilter({
      status: ["submitted"],
      submitter_id: null,
      limit: 200,
      offset: 0,
    });
  }, [searchParams, setFilter, setView]);

  useEffect(() => {
    const raw = searchParams.get("openDi");
    if (raw) {
      const id = Number.parseInt(raw, 10);
      if (!Number.isNaN(id)) {
        void openDi(id);
        setSearchParams(
          (prev) => {
            const p = new URLSearchParams(prev);
            p.delete("openDi");
            return p;
          },
          { replace: true },
        );
      }
    }
  }, [searchParams, openDi, setSearchParams]);

  useEffect(() => {
    void loadDis();
  }, [loadDis, searchParams]);

  useEffect(() => {
    if (loading) return;
    setArchiveRefreshKey((k) => k + 1);
  }, [loading]);

  const switchView = useCallback((v: ViewMode) => {
    setView(v);
    localStorage.setItem("di-view-mode", v);
  }, []);

  const handleCardClick = useCallback((di: InterventionRequest) => void openDi(di.id), [openDi]);

  const STATUS_OPTIONS = useMemo(
    () =>
      Object.keys(DI_STATUS_TONE).map((code) => ({
        value: code,
        label: t(`status.${diStatusToI18nKey(code)}` as const),
      })),
    [t],
  );

  const PRIORITY_OPTIONS = useMemo(
    () =>
      ["low", "medium", "high", "critical"].map((code) => ({
        value: code,
        label: t(`priority.${code}`),
      })),
    [t],
  );

  const onSearchChange = useCallback(
    (val: string) => {
      setFilter({ search: val.trim() || null });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleStatusFilter = useCallback(
    (val: string | null) => {
      setStatusFilter(val);
      if (val === STATUS_FILTER_REVIEW_QUEUE) {
        setFilter({
          status: [...DI_REVIEW_QUEUE_STATUSES],
          submitter_id: null,
          limit: 200,
          offset: 0,
        });
        setSearchParams(
          (prev) => {
            const p = new URLSearchParams(prev);
            p.set("review", "1");
            p.delete("triage");
            return p;
          },
          { replace: true },
        );
      } else if (val === STATUS_FILTER_TRIAGE_INBOX) {
        setFilter({
          status: ["submitted"],
          submitter_id: null,
          limit: 200,
          offset: 0,
        });
        setSearchParams(
          (prev) => {
            const p = new URLSearchParams(prev);
            p.delete("review");
            p.set("triage", "1");
            return p;
          },
          { replace: true },
        );
      } else {
        if (searchParams.get("review")) {
          setSearchParams(
            (prev) => {
              const p = new URLSearchParams(prev);
              p.delete("review");
              return p;
            },
            { replace: true },
          );
        }
        if (searchParams.get("triage")) {
          setSearchParams(
            (prev) => {
              const p = new URLSearchParams(prev);
              p.delete("triage");
              return p;
            },
            { replace: true },
          );
        }
        if (val == null) {
          setFilter({ status: null, submitter_id: null, limit: 50, offset: 0 });
        } else {
          setFilter({ status: [val], submitter_id: null, limit: 50, offset: 0 });
        }
      }
      void loadDis();
    },
    [loadDis, setFilter, searchParams, setSearchParams],
  );

  const handlePriorityFilter = useCallback(
    (val: string | null) => {
      setPriorityFilter(val);
      setFilter({ urgency: val });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const handleDispositionFilter = useCallback(
    (val: string | null) => {
      setDispositionFilter(val);
      setFilter({ disposition_code: val });
      void loadDis();
    },
    [loadDis, setFilter],
  );

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setStatusFilter(null);
    setPriorityFilter(null);
    setDispositionFilter(null);
    setSearchParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        p.delete("review");
        p.delete("triage");
        return p;
      },
      { replace: true },
    );
    setFilter({
      search: null,
      status: null,
      urgency: null,
      disposition_code: null,
      submitter_id: null,
      limit: 50,
      offset: 0,
    });
    void loadDis();
  }, [loadDis, setFilter, setSearchParams]);

  const statusFilterOptions = useMemo(() => {
    return [
      { value: STATUS_FILTER_REVIEW_QUEUE, label: t("list.filters.reviewQueue") },
      ...(can(P.DI_SCREEN) || can(P.DI_REVIEW)
        ? [{ value: STATUS_FILTER_TRIAGE_INBOX, label: t("list.filters.triageInbox") }]
        : []),
      ...STATUS_OPTIONS,
    ];
  }, [STATUS_OPTIONS, can, t]);

  const DISPOSITION_OPTIONS = useMemo(
    () =>
      DI_DISPOSITION_FILTER_CODES.map((code) => ({
        value: code,
        label: t(`disposition.${code}` as const),
      })),
    [t],
  );

  const filterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "status",
        kind: "select",
        label: t("list.filters.status"),
        options: statusFilterOptions,
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
    ],
    [
      t,
      statusFilterOptions,
      PRIORITY_OPTIONS,
      DISPOSITION_OPTIONS,
      statusFilter,
      priorityFilter,
      dispositionFilter,
      handleStatusFilter,
      handlePriorityFilter,
      handleDispositionFilter,
    ],
  );

  const toggleFilters = useCallback(() => {
    setShowFilters((prev) => {
      const next = !prev;
      localStorage.setItem("di-show-filters", next ? "1" : "0");
      return next;
    });
  }, []);

  const columns: ColumnDef<InterventionRequest>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => (
          <span className="font-mono text-xs inline-flex items-center gap-1.5 flex-wrap">
            {row.original.code}
            {row.original.is_modified && <StatusChip label={t("review.modified")} tone="warning" />}
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.converted_to_wo_code}
              entityId={row.original.converted_to_wo_id}
              title={row.original.converted_to_wo_title}
              className="text-[10px] px-1.5 py-0"
            />
          </span>
        ),
      },
      {
        accessorKey: "title",
        header: t("list.columns.subject"),
        cell: ({ row }) => (
          <span className="max-w-[200px] truncate block">{row.original.title}</span>
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
        cell: ({ row }) => {
          const s = row.original.status;
          return (
            <StatusChip
              label={t(`status.${diStatusToI18nKey(s)}` as const)}
              tone={toneOf(DI_STATUS_TONE, s)}
              dot
            />
          );
        },
      },
      {
        accessorKey: "reported_urgency",
        header: t("list.columns.priority"),
        cell: ({ row }) => {
          const u = row.original.reported_urgency;
          return <StatusChip label={t(`priority.${u}`)} tone={toneOf(DI_URGENCY_TONE, u)} />;
        },
      },
      {
        accessorKey: "submitted_at",
        header: t("list.columns.reportedAt"),
        cell: ({ row }) => {
          const d = row.original.submitted_at;
          return (
            <span className="text-xs text-text-muted">
              {formatDiDate(d, dateLocale, {
                day: "2-digit",
                month: "2-digit",
                year: "numeric",
              })}
            </span>
          );
        },
      },
    ],
    [t, dateLocale],
  );

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

  return (
    <ModulePageShell
      icon={ClipboardList}
      title={t("page.title")}
      count={total}
      bodyClassName={null}
      actions={
        <>
          <PermissionGate permission={P.DI_CREATE}>
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
          {can(P.DI_ADMIN) && (
            <Button variant="outline" className="h-11" onClick={() => setSlaOpen(true)}>
              {t("sla.title")}
            </Button>
          )}
          <RefreshButton onRefresh={() => void loadDis()} loading={loading} />
        </>
      }
      filterBar={
        <>
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
          {/* Review queue (approvers only) */}
          <PermissionGate anyOf={[P.DI_SCREEN, P.DI_REVIEW]}>
            <DiReviewPanel />
          </PermissionGate>
        </>
      }
    >
      {recordView && showListError ? (
        <div className="p-4">
          <ErrorState error={listError} onRetry={() => void loadDis()} retrying={loading} />
        </div>
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
      {view === "list" && !showListError && (
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
      {view === "kanban" && !showListError && (
        <DiKanbanBoard items={items} onCardClick={handleCardClick} />
      )}
      {view === "calendar" && !showListError && (
        <DiCalendarView items={items} onChipClick={(di) => void openDi(di.id)} />
      )}
      {view === "dashboard" && <DiDashboardView />}

      {/* ── Archive section (collapsible) ─────────────────────────────── */}
      <DiArchivePanel refreshKey={archiveRefreshKey} onRowClick={(di) => void openDi(di.id)} />

      {/* ── Floating detail dialog ───────────────────────────────────── */}
      <DiDetailDialog
        di={activeDi?.di ?? null}
        transitions={activeDi?.transitions ?? []}
        open={activeDi !== null}
        onClose={closeDi}
      />

      <DiFormDialog />

      {/* Review dialogs */}
      <DiApprovalDialog />
      <DiCloseDialog />
      <DiReturnDialog />

      {can(P.DI_ADMIN) && <DiSlaRulesPanel open={slaOpen} onClose={() => setSlaOpen(false)} />}
    </ModulePageShell>
  );
}

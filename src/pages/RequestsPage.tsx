/**
 * RequestsPage.tsx
 *
 * Multi-view intervention-request (DI) workspace inspired by the web version.
 * Views: List (DataTable) and Kanban board, toggled via header buttons.
 * Detail: floating dialog (DiDetailDialog) — the underlying view stays fully
 * visible and usable when the dialog is closed.
 */

import type { ColumnDef } from "@tanstack/react-table";
import {
  BarChart3,
  CalendarDays,
  ClipboardList,
  Columns3,
  Filter,
  List,
  Plus,
  RefreshCw,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { DiApprovalDialog } from "@/components/di/DiApprovalDialog";
import { DiArchivePanel } from "@/components/di/DiArchivePanel";
import { DiCalendarView } from "@/components/di/DiCalendarView";
import { DiDashboardView } from "@/components/di/DiDashboardView";
import { DiDetailDialog } from "@/components/di/DiDetailDialog";
import { DiFormDialog } from "@/components/di/DiFormDialog";
import { DiKanbanBoard } from "@/components/di/DiKanbanBoard";
import { DiRejectionDialog } from "@/components/di/DiRejectionDialog";
import { DiReturnDialog } from "@/components/di/DiReturnDialog";
import { DiReviewPanel } from "@/components/di/DiReviewPanel";
import { DiSlaRulesPanel } from "@/components/di/DiSlaRulesPanel";
import { DI_STATUS_STYLE, diStatusToI18nKey } from "@/components/di/status-meta";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { useDiStore } from "@/stores/di-store";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

const URGENCY_STYLE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

type ViewMode = "list" | "kanban" | "calendar" | "dashboard";

/** Same cohort as `loadReviewQueue` in `di-review-store` (pending triage + approval). */
const DI_REVIEW_QUEUE_STATUSES = [
  "pending_review",
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
  const activeDi = useDiStore((s) => s.activeDi);
  const loadDis = useDiStore((s) => s.loadDis);
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

  const switchView = useCallback((v: ViewMode) => {
    setView(v);
    localStorage.setItem("di-view-mode", v);
  }, []);

  const handleCardClick = useCallback((di: InterventionRequest) => void openDi(di.id), [openDi]);

  const STATUS_OPTIONS = useMemo(
    () =>
      Object.keys(DI_STATUS_STYLE).map((code) => ({
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

  const resetFilters = useCallback(() => {
    setSearchInput("");
    setStatusFilter(null);
    setPriorityFilter(null);
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
      submitter_id: null,
      limit: 50,
      offset: 0,
    });
    void loadDis();
  }, [loadDis, setFilter, setSearchParams]);

  const statusFilterOptions = useMemo(() => {
    return [
      { value: STATUS_FILTER_REVIEW_QUEUE, label: t("list.filters.reviewQueue") },
      ...(can("di.screen") || can("di.review")
        ? [{ value: STATUS_FILTER_TRIAGE_INBOX, label: t("list.filters.triageInbox") }]
        : []),
      ...STATUS_OPTIONS,
    ];
  }, [STATUS_OPTIONS, can, t]);

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
    ],
    [
      t,
      statusFilterOptions,
      PRIORITY_OPTIONS,
      statusFilter,
      priorityFilter,
      handleStatusFilter,
      handlePriorityFilter,
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
            {row.original.is_modified && (
              <Badge className="bg-amber-100 text-amber-800 border-0 text-[9px] px-1 py-0">
                {t("review.modified")}
              </Badge>
            )}
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
        accessorKey: "status",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.status;
          return (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ${DI_STATUS_STYLE[s] ?? "bg-gray-100 text-gray-600"}`}
            >
              {t(`status.${diStatusToI18nKey(s)}` as const)}
            </Badge>
          );
        },
      },
      {
        accessorKey: "reported_urgency",
        header: t("list.columns.priority"),
        cell: ({ row }) => {
          const u = row.original.reported_urgency;
          return (
            <Badge variant="outline" className={`text-[10px] border-0 ${URGENCY_STYLE[u] ?? ""}`}>
              {t(`priority.${u}`)}
            </Badge>
          );
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
    [t],
  );

  return (
    <div className={mfLayout.moduleRoot}>
      {/* ── Page header ──────────────────────────────────────────────── */}
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <ClipboardList className={mfLayout.moduleHeaderIcon} />
          <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className={mfLayout.moduleHeaderActions}>
          {/* New DI button */}
          <PermissionGate permission="di.create">
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

          {can("di.admin") && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSlaOpen(true)}
              title={t("sla.title")}
              className="gap-1.5"
            >
              {t("sla.title")}
            </Button>
          )}

          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadDis()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </div>

      {showFilters && (
        <SmartFilterBar
          searchPlaceholder={t("search.placeholder")}
          searchValue={searchInput}
          onSearchInputChange={setSearchInput}
          onSearchChange={onSearchChange}
          filters={filterDefs}
          resultCount={items.length}
          onReset={resetFilters}
        />
      )}

      {/* ── Review panel (approvers only) ────────────────────────── */}
      <PermissionGate anyOf={["di.screen", "di.review"]}>
        <DiReviewPanel />
      </PermissionGate>

      {/* ── Main workspace ───────────────────────────────────────────── */}
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          {view === "list" && (
            <div className="p-4">
              <DataTable
                columns={columns}
                data={items}
                searchable
                pageSize={20}
                isLoading={loading}
                skeletonRows={8}
                onRowClick={(row) => void openDi(row.id)}
              />
            </div>
          )}
          {view === "kanban" && <DiKanbanBoard items={items} onCardClick={handleCardClick} />}
          {view === "calendar" && (
            <DiCalendarView items={items} onChipClick={(di) => void openDi(di.id)} />
          )}
          {view === "dashboard" && <DiDashboardView />}
        </div>
      </div>

      {/* ── Archive section (collapsible) ─────────────────────────────── */}
      <DiArchivePanel />

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
      <DiRejectionDialog />
      <DiReturnDialog />

      {can("di.admin") && <DiSlaRulesPanel open={slaOpen} onClose={() => setSlaOpen(false)} />}
    </div>
  );
}

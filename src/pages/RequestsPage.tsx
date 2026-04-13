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
  List,
  Plus,
  RefreshCw,
  Settings,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { usePermissions } from "@/hooks/use-permissions";
import { useDiStore } from "@/stores/di-store";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Status → badge style mapping ────────────────────────────────────────────

const STATUS_STYLE: Record<string, string> = {
  submitted: "bg-blue-100 text-blue-800",
  pending_review: "bg-amber-100 text-amber-800",
  returned_for_clarification: "bg-orange-100 text-orange-800",
  rejected: "bg-red-100 text-red-700",
  screened: "bg-sky-100 text-sky-800",
  awaiting_approval: "bg-yellow-100 text-yellow-800",
  approved_for_planning: "bg-green-100 text-green-800",
  deferred: "bg-gray-100 text-gray-600",
  converted_to_work_order: "bg-emerald-100 text-emerald-800",
  closed_as_non_executable: "bg-slate-100 text-slate-600",
  archived: "bg-neutral-100 text-neutral-500",
};

const URGENCY_STYLE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

type ViewMode = "list" | "kanban" | "calendar" | "dashboard";

// ── Component ───────────────────────────────────────────────────────────────

export function RequestsPage() {
  const { t } = useTranslation("di");
  const items = useDiStore((s) => s.items);
  const total = useDiStore((s) => s.total);
  const loading = useDiStore((s) => s.loading);
  const activeDi = useDiStore((s) => s.activeDi);
  const loadDis = useDiStore((s) => s.loadDis);
  const openDi = useDiStore((s) => s.openDi);
  const closeDi = useDiStore((s) => s.closeDi);
  const openCreateForm = useDiStore((s) => s.openCreateForm);

  const [view, setView] = useState<ViewMode>(
    () => (localStorage.getItem("di-view-mode") as ViewMode) || "kanban",
  );

  const { can } = usePermissions();
  const [slaOpen, setSlaOpen] = useState(false);

  useEffect(() => {
    void loadDis();
  }, [loadDis]);

  const switchView = useCallback((v: ViewMode) => {
    setView(v);
    localStorage.setItem("di-view-mode", v);
  }, []);

  const handleCardClick = useCallback((di: InterventionRequest) => void openDi(di.id), [openDi]);

  const columns: ColumnDef<InterventionRequest>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => (
          <span className="font-mono text-xs">
            {row.original.code}
            {row.original.is_modified && (
              <Badge className="ml-1.5 bg-amber-100 text-amber-800 border-0 text-[9px] px-1 py-0">
                {t("review.modified")}
              </Badge>
            )}
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
              className={`text-[10px] border-0 ${STATUS_STYLE[s] ?? "bg-gray-100 text-gray-600"}`}
            >
              {t(`status.${statusToI18nKey(s)}` as const)}
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
          return <span className="text-xs text-text-muted">{formatDate(d)}</span>;
        },
      },
    ],
    [t],
  );

  return (
    <div className="flex h-full flex-col">
      {/* ── Page header ──────────────────────────────────────────────── */}
      <div className="flex items-center justify-between px-6 py-3 border-b border-surface-border">
        <div className="flex items-center gap-3">
          <ClipboardList className="h-5 w-5 text-text-muted" />
          <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          {/* New DI button */}
          <PermissionGate permission="di.create">
            <Button size="sm" onClick={() => openCreateForm()} className="gap-1.5">
              <Plus className="h-3.5 w-3.5" />
              {t("action.create")}
            </Button>
          </PermissionGate>

          {/* View toggle */}
          <div className="flex items-center rounded-md border p-0.5 gap-0.5">
            <Button
              variant={view === "list" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("list")}
              title="Vue liste"
            >
              <List className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "kanban" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("kanban")}
              title="Vue Kanban"
            >
              <Columns3 className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "calendar" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("calendar")}
              title="Vue calendrier"
            >
              <CalendarDays className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "dashboard" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("dashboard")}
              title="Tableau de bord"
            >
              <BarChart3 className="h-3.5 w-3.5" />
            </Button>
          </div>

          {can("di.admin") && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSlaOpen(true)}
              title={t("sla.title")}
              className="gap-1.5"
            >
              <Settings className="h-3.5 w-3.5" />
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

      {/* ── Review panel (approvers only) ────────────────────────── */}
      <PermissionGate permission="di.review">
        <DiReviewPanel />
      </PermissionGate>

      {/* ── Main workspace ───────────────────────────────────────────── */}
      <div className="flex flex-1 min-h-0">
        <div className="flex flex-col w-full overflow-auto">
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
      <DiDetailDialog di={activeDi?.di ?? null} open={activeDi !== null} onClose={closeDi} />

      <DiFormDialog />

      {/* Review dialogs */}
      <DiApprovalDialog />
      <DiRejectionDialog />
      <DiReturnDialog />

      {can("di.admin") && <DiSlaRulesPanel open={slaOpen} onClose={() => setSlaOpen(false)} />}
    </div>
  );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

type DiStatusKey =
  | "new"
  | "inReview"
  | "approved"
  | "rejected"
  | "inProgress"
  | "resolved"
  | "closed"
  | "cancelled";

/** Map Rust snake_case status to camelCase i18n key */
function statusToI18nKey(s: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    submitted: "new",
    pending_review: "inReview",
    returned_for_clarification: "inReview",
    rejected: "rejected",
    screened: "inReview",
    awaiting_approval: "inReview",
    approved_for_planning: "approved",
    deferred: "inReview",
    converted_to_work_order: "inProgress",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[s] ?? "new";
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString("fr-FR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}

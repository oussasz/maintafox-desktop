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
  Plus,
  RefreshCw,
  Search,
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
import { WoArchivePanel } from "@/components/wo/WoArchivePanel";
import { WoCalendarView } from "@/components/wo/WoCalendarView";
import { WoDashboardView } from "@/components/wo/WoDashboardView";
import { WoDetailDialog } from "@/components/wo/WoDetailDialog";
import { WoDiManagementPanel } from "@/components/wo/WoDiManagementPanel";
import { WoFormDialog } from "@/components/wo/WoFormDialog";
import { WoKanbanBoard } from "@/components/wo/WoKanbanBoard";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

// ── Status → badge style mapping ────────────────────────────────────────────

const STATUS_STYLE: Record<string, string> = {
  draft: "bg-gray-100 text-gray-600",
  planned: "bg-blue-100 text-blue-800",
  released: "bg-sky-100 text-sky-800",
  in_progress: "bg-amber-100 text-amber-800",
  on_hold: "bg-orange-100 text-orange-800",
  completed: "bg-green-100 text-green-800",
  verified: "bg-teal-100 text-teal-800",
  closed: "bg-neutral-100 text-neutral-500",
  cancelled: "bg-red-100 text-red-700",
};

type WoViewMode = "list" | "kanban" | "calendar" | "dashboard";

// ── Component ───────────────────────────────────────────────────────────────

export function WorkOrdersPage() {
  const { t } = useTranslation("ot");
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
    () => (localStorage.getItem("wo-view-mode") as WoViewMode) || "list",
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

  // ── Load on mount ─────────────────────────────────────────────────────

  useEffect(() => {
    void loadWos();
  }, [loadWos]);

  const switchView = useCallback((v: WoViewMode) => {
    setView(v);
    localStorage.setItem("wo-view-mode", v);
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
            {row.original.planned_end ? formatDate(row.original.planned_end) : "—"}
          </span>
        ),
      },
    ],
    [t],
  );

  return (
    <div className="flex h-full flex-col">
      {/* ── Page header ──────────────────────────────────────────────── */}
      <div className="flex items-center justify-between px-6 py-3 border-b border-surface-border">
        <div className="flex items-center gap-3">
          <Wrench className="h-5 w-5 text-text-muted" />
          <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
          <Badge variant="secondary" className="text-xs">
            {total}
          </Badge>
        </div>

        <div className="flex items-center gap-2">
          {/* New WO button */}
          <PermissionGate permission="ot.create">
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
              title="List"
            >
              <List className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "kanban" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("kanban")}
              title="Kanban"
            >
              <Columns3 className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "calendar" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("calendar")}
              title="Calendar"
            >
              <CalendarDays className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant={view === "dashboard" ? "default" : "ghost"}
              size="sm"
              className="h-7 px-2"
              onClick={() => switchView("dashboard")}
              title="Dashboard"
            >
              <BarChart3 className="h-3.5 w-3.5" />
            </Button>
          </div>

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
      <div className="flex items-center gap-2 px-6 py-2 border-b border-surface-border">
        <div className="relative flex-1 max-w-sm">
          <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            className="pl-9 h-8 text-sm"
            placeholder={t("form.equipment.placeholder")}
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
      </div>

      {/* ── DI management panel (ot.edit permission) ─────────────────── */}
      <PermissionGate permission="ot.edit">
        <WoDiManagementPanel />
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
                onRowClick={(row) => void openWo(row.id)}
              />
            </div>
          )}
          {view === "kanban" && (
            <WoKanbanBoard items={items} onCardClick={(wo) => void openWo(wo.id)} />
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

// ── Helpers ──────────────────────────────────────────────────────────────────

type WoStatusKey =
  | "draft"
  | "planned"
  | "released"
  | "inProgress"
  | "onHold"
  | "completed"
  | "verified"
  | "closed"
  | "cancelled";

function statusToI18nKey(s: string): WoStatusKey {
  const map: Record<string, WoStatusKey> = {
    draft: "draft",
    planned: "planned",
    released: "released",
    in_progress: "inProgress",
    on_hold: "onHold",
    completed: "completed",
    verified: "verified",
    closed: "closed",
    cancelled: "cancelled",
  };
  return map[s] ?? "draft";
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

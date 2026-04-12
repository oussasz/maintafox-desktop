/**
 * WoKanbanView.tsx
 *
 * Kanban board grouped by work_order_statuses.macro_state.
 * Columns: Open | Executing | Completed | Closed | Cancelled
 *
 * Each card shows: code, title, asset label, urgency colour bar,
 * assignee name, planned_end.
 *
 * Cards are draggable. Drop triggers the appropriate transition command
 * (validated against allowed_transitions before any invoke call).
 * If the transition is invalid an error toast is shown instead.
 *
 * Each column shows up to PAGE_SIZE (20) cards with a "Load more" button.
 *
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S3.
 */

import { AlertTriangle, Calendar as CalendarIcon, ChevronDown, User, XCircle } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useSession } from "@/hooks/use-session";
import { cn } from "@/lib/utils";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

// ── Constants ─────────────────────────────────────────────────────────────────

const PAGE_SIZE = 20;

/**
 * status_code → macro_state mapping.
 * Derived from work_order_statuses seeded data (migration 021).
 */
const STATUS_TO_MACRO: Record<string, string> = {
  draft: "open",
  awaiting_approval: "open",
  planned: "open",
  ready_to_schedule: "open",
  assigned: "open",
  waiting_for_prerequisite: "open",
  in_progress: "executing",
  on_hold: "executing",
  paused: "executing",
  mechanically_complete: "completed",
  technically_verified: "completed",
  closed: "closed",
  cancelled: "cancelled",
};

/**
 * allowed_transitions mirrors the Rust domain guard_wo_transition table.
 * Each key = from_status; value = valid to_statuses.
 * Used to validate drag before any invoke call.
 */
const ALLOWED_TRANSITIONS: Record<string, string[]> = {
  draft: ["awaiting_approval", "planned", "cancelled"],
  awaiting_approval: ["planned", "cancelled"],
  planned: ["ready_to_schedule", "cancelled"],
  ready_to_schedule: ["assigned", "waiting_for_prerequisite", "cancelled"],
  assigned: ["waiting_for_prerequisite", "in_progress", "cancelled"],
  waiting_for_prerequisite: ["assigned", "in_progress", "cancelled"],
  in_progress: ["paused", "on_hold", "mechanically_complete", "cancelled"],
  on_hold: ["in_progress", "cancelled"],
  paused: ["in_progress", "cancelled"],
  mechanically_complete: ["technically_verified"],
  technically_verified: ["closed", "in_progress"],
  closed: [],
  cancelled: [],
};

// ── Column definitions ────────────────────────────────────────────────────────

interface ColumnDef {
  id: string; // macro_state value
  labelKey: string;
  headerClass: string;
  emptyKey: string;
}

const COLUMNS: ColumnDef[] = [
  {
    id: "open",
    labelKey: "kanban.columnOpen",
    headerClass: "bg-blue-50 border-blue-200 text-blue-800",
    emptyKey: "kanban.emptyOpen",
  },
  {
    id: "executing",
    labelKey: "kanban.columnExecuting",
    headerClass: "bg-amber-50 border-amber-200 text-amber-800",
    emptyKey: "kanban.emptyExecuting",
  },
  {
    id: "completed",
    labelKey: "kanban.columnCompleted",
    headerClass: "bg-teal-50 border-teal-200 text-teal-800",
    emptyKey: "kanban.emptyCompleted",
  },
  {
    id: "closed",
    labelKey: "kanban.columnClosed",
    headerClass: "bg-neutral-50 border-neutral-200 text-neutral-600",
    emptyKey: "kanban.emptyClosed",
  },
  {
    id: "cancelled",
    labelKey: "kanban.columnCancelled",
    headerClass: "bg-red-50 border-red-200 text-red-700",
    emptyKey: "kanban.emptyCancelled",
  },
];

// ── Urgency styling ───────────────────────────────────────────────────────────

const URGENCY_BAR: Record<number, string> = {
  1: "bg-emerald-400",
  2: "bg-blue-400",
  3: "bg-amber-400",
  4: "bg-orange-500",
  5: "bg-red-600",
};

function urgencyBar(urgencyId: number | null | undefined) {
  const cls = URGENCY_BAR[urgencyId ?? 0] ?? "bg-gray-300";
  return <div className={cn("h-1 w-full rounded-t-sm", cls)} />;
}

function shortDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      day: "2-digit",
      month: "short",
    });
  } catch {
    return iso;
  }
}

// ── Toast helper ──────────────────────────────────────────────────────────────
// Minimal inline toast — no Provider needed for ephemeral error toasts.

interface InlineToast {
  id: number;
  message: string;
}

// ── WO Card ───────────────────────────────────────────────────────────────────

interface WoCardProps {
  wo: WorkOrder;
  onCardClick: (wo: WorkOrder) => void;
  onDragStart: (e: React.DragEvent, wo: WorkOrder) => void;
}

function WoCard({ wo, onCardClick, onDragStart }: WoCardProps) {
  const { t } = useTranslation("ot");
  return (
    <div
      draggable
      tabIndex={0}
      role="button"
      aria-label={`${wo.code}: ${wo.title}`}
      className="group cursor-grab rounded-md border border-surface-border bg-background shadow-sm hover:shadow-md focus:outline-none focus:ring-2 focus:ring-ring active:cursor-grabbing"
      onClick={() => onCardClick(wo)}
      onKeyDown={(e) => e.key === "Enter" && onCardClick(wo)}
      onDragStart={(e) => onDragStart(e, wo)}
    >
      {/* Urgency colour bar */}
      {urgencyBar(wo.urgency_id)}

      <div className="px-3 pb-3 pt-2 space-y-1.5">
        {/* Code + urgency label */}
        <div className="flex items-center justify-between gap-1">
          <span className="font-mono text-[11px] text-muted-foreground">{wo.code}</span>
          {wo.urgency_label && (
            <Badge
              variant="outline"
              className="text-[10px] px-1 py-0 border-0 bg-transparent text-muted-foreground"
            >
              {wo.urgency_label}
            </Badge>
          )}
        </div>

        {/* Title */}
        <p className="text-xs font-medium leading-tight line-clamp-2">{wo.title}</p>

        {/* Asset */}
        {wo.asset_label && (
          <p className="text-[11px] text-muted-foreground truncate">{wo.asset_label}</p>
        )}

        {/* Footer: assignee + planned_end */}
        <div className="flex items-center justify-between gap-2 pt-0.5 text-[10px] text-muted-foreground">
          <span className="flex items-center gap-0.5 truncate">
            <User className="h-2.5 w-2.5 shrink-0" />
            <span className="truncate">{wo.responsible_username ?? t("kanban.unassigned")}</span>
          </span>
          <span className="flex items-center gap-0.5 shrink-0">
            <CalendarIcon className="h-2.5 w-2.5" />
            {shortDate(wo.planned_end)}
          </span>
        </div>
      </div>
    </div>
  );
}

// ── Kanban column ─────────────────────────────────────────────────────────────

interface KanbanColumnProps {
  col: ColumnDef;
  items: WorkOrder[];
  onCardClick: (wo: WorkOrder) => void;
  onDragStart: (e: React.DragEvent, wo: WorkOrder) => void;
  onDrop: (e: React.DragEvent, targetMacroState: string) => void;
}

function KanbanColumn({ col, items, onCardClick, onDragStart, onDrop }: KanbanColumnProps) {
  const { t } = useTranslation("ot");
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [dragOver, setDragOver] = useState(false);

  const visible = items.slice(0, visibleCount);
  const hasMore = items.length > visibleCount;

  return (
    <div
      className={cn(
        "flex flex-col min-w-[220px] max-w-[260px] flex-1 rounded-lg border",
        dragOver ? "ring-2 ring-primary ring-offset-1" : "",
        col.headerClass.includes("border") ? "" : "border-surface-border",
      )}
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(e) => {
        setDragOver(false);
        onDrop(e, col.id);
      }}
    >
      {/* Column header */}
      <div
        className={cn(
          "flex items-center justify-between px-3 py-2 rounded-t-lg border-b",
          col.headerClass,
        )}
      >
        <span className="text-xs font-semibold">{t(col.labelKey)}</span>
        <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-0 bg-white/60">
          {items.length}
        </Badge>
      </div>

      {/* Cards */}
      <div className="flex-1 overflow-y-auto p-2 space-y-2 min-h-[120px]">
        {visible.length === 0 ? (
          <p className="py-4 text-center text-[11px] text-muted-foreground">{t(col.emptyKey)}</p>
        ) : (
          visible.map((wo) => (
            <WoCard key={wo.id} wo={wo} onCardClick={onCardClick} onDragStart={onDragStart} />
          ))
        )}
      </div>

      {/* Load more */}
      {hasMore && (
        <div className="border-t p-2">
          <Button
            variant="ghost"
            size="sm"
            className="w-full text-xs gap-1"
            onClick={() => setVisibleCount((n) => n + PAGE_SIZE)}
          >
            <ChevronDown className="h-3.5 w-3.5" />
            {t("kanban.loadMore", { count: items.length - visibleCount })}
          </Button>
        </div>
      )}
    </div>
  );
}

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoKanbanViewProps {
  items: WorkOrder[];
  onCardClick: (wo: WorkOrder) => void;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoKanbanView({ items, onCardClick }: WoKanbanViewProps) {
  const { t } = useTranslation("ot");
  const [toasts, setToasts] = useState<InlineToast[]>([]);
  const toastCounter = useRef(0);
  const draggedWo = useRef<WorkOrder | null>(null);
  const { info: session } = useSession();
  const assignWorkOrder = useWoStore((s) => s.assignWorkOrder);

  const addToast = useCallback((message: string) => {
    const id = ++toastCounter.current;
    setToasts((prev) => [...prev, { id, message }]);
    setTimeout(() => setToasts((prev) => prev.filter((t) => t.id !== id)), 4000);
  }, []);

  // Group items by macro_state
  const grouped = new Map<string, WorkOrder[]>(COLUMNS.map((c) => [c.id, []]));
  for (const wo of items) {
    const macro = STATUS_TO_MACRO[wo.status_code ?? "draft"] ?? "open";
    grouped.get(macro)?.push(wo);
  }

  const handleDragStart = useCallback((e: React.DragEvent, wo: WorkOrder) => {
    draggedWo.current = wo;
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", String(wo.id));
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent, targetMacroState: string) => {
      e.preventDefault();
      const wo = draggedWo.current;
      draggedWo.current = null;
      if (!wo) return;

      const fromMacro = STATUS_TO_MACRO[wo.status_code ?? "draft"] ?? "open";
      if (fromMacro === targetMacroState) return; // dropped on same column

      // Determine target status candidates for the target macro_state
      const targetStatuses = Object.entries(STATUS_TO_MACRO)
        .filter(([, macro]) => macro === targetMacroState)
        .map(([s]) => s);

      // Check if ANY transition from current status_code to ANY target status is valid
      const currentStatus = wo.status_code ?? "draft";
      const validTargets = (ALLOWED_TRANSITIONS[currentStatus] ?? []).filter((to) =>
        targetStatuses.includes(to),
      );

      if (validTargets.length === 0) {
        addToast(
          t("kanban.transitionNotAllowed", {
            code: wo.code,
            from: currentStatus.replace(/_/g, " "),
            to: COLUMNS.find((c) => c.id === targetMacroState)?.id ?? targetMacroState,
          }),
        );
        return;
      }

      // For an "open" drop targeting "assigned", invoke assignWorkOrder if we have a responsible.
      // For other valid transitions, open the WO detail dialog to let the user apply the correct action.
      // This keeps the Kanban "visual only" for complex transitions and direct for assign.
      if (
        targetMacroState === "open" &&
        validTargets.includes("assigned") &&
        wo.primary_responsible_id &&
        session?.user_id
      ) {
        void assignWorkOrder({
          wo_id: wo.id,
          actor_id: session.user_id,
          expected_row_version: wo.row_version,
          primary_responsible_id: wo.primary_responsible_id,
        }).catch((err: unknown) => {
          addToast(String(err));
        });
      } else {
        // Open detail dialog to execute the transition with full context
        onCardClick(wo);
      }
    },
    [addToast, assignWorkOrder, onCardClick, session.user_id, t],
  );

  return (
    <div className="relative flex flex-col h-full">
      {/* Inline error toasts */}
      {toasts.length > 0 && (
        <div className="absolute top-0 right-0 z-50 flex flex-col gap-2 p-3 max-w-sm">
          {toasts.map((toast) => (
            <div
              key={toast.id}
              className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive shadow-md"
            >
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>{toast.message}</span>
              <button
                className="ml-auto shrink-0 opacity-70 hover:opacity-100"
                onClick={() => setToasts((p) => p.filter((x) => x.id !== toast.id))}
              >
                <XCircle className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Board */}
      <div className="flex gap-3 flex-1 overflow-x-auto p-1 pb-2">
        {COLUMNS.map((col) => (
          <KanbanColumn
            key={col.id}
            col={col}
            items={grouped.get(col.id) ?? []}
            onCardClick={onCardClick}
            onDragStart={handleDragStart}
            onDrop={handleDrop}
          />
        ))}
      </div>
    </div>
  );
}

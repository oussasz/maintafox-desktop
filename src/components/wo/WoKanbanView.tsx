/**
 * WoKanbanView.tsx
 *
 * Data adapter for work-order lanes. Owns status→column mapping and open-detail.
 *
 * Cards are not draggable: every lifecycle step has its own checks and inputs (submit,
 * readiness, permit gate on start, delay reason on hold, completion gates, cancel reason),
 * so status changes are made from the OT detail footer, not by dropping a card. The board
 * says so visibly instead of offering a drop that silently does something else.
 */

import { Calendar as CalendarIcon, User } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { KanbanBoard } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn, KanbanTone } from "@/components/kanban";
import type { WorkOrder } from "@shared/ipc-types";

/**
 * Option B status_code → column id mapping.
 * Legacy codes from old data are mapped to nearest equivalent for display.
 */
const STATUS_TO_COLUMN: Record<string, string> = {
  draft: "draft",
  planning: "planning",
  ready: "ready",
  in_progress: "executing",
  on_hold: "executing",
  completed: "completed",
  closed: "closed",
  cancelled: "cancelled",
  awaiting_approval: "planning",
  planned: "planning",
  ready_to_schedule: "ready",
  assigned: "ready",
  waiting_for_prerequisite: "executing",
  paused: "executing",
  mechanically_complete: "completed",
  technically_verified: "completed",
};

const COLUMN_META: { id: string; labelKey: string; tone: KanbanTone }[] = [
  { id: "draft", labelKey: "kanban.columnDraft", tone: "draft" },
  { id: "planning", labelKey: "kanban.columnPlanning", tone: "planning" },
  { id: "ready", labelKey: "kanban.columnReady", tone: "ready" },
  { id: "executing", labelKey: "kanban.columnExecuting", tone: "executing" },
  { id: "completed", labelKey: "kanban.columnCompleted", tone: "success" },
  { id: "closed", labelKey: "kanban.columnClosed", tone: "closed" },
  { id: "cancelled", labelKey: "kanban.columnCancelled", tone: "cancelled" },
];

/** Same urgency tone vocabulary as DI. */
const URGENCY_TONE: Record<number, KanbanTone> = {
  1: "low",
  2: "medium",
  3: "medium",
  4: "high",
  5: "critical",
};

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

interface WoKanbanViewProps {
  items: WorkOrder[];
  onCardClick: (wo: WorkOrder) => void;
}

export function WoKanbanView({ items, onCardClick }: WoKanbanViewProps) {
  const { t } = useTranslation(["ot", "common"]);
  const byId = useRef(new Map<string, WorkOrder>());

  const columns = useMemo<KanbanColumn[]>(
    () =>
      COLUMN_META.map((c) => ({
        id: c.id,
        label: t(c.labelKey),
        tone: c.tone,
      })),
    [t],
  );

  const cards = useMemo<KanbanCard[]>(() => {
    const map = new Map<string, WorkOrder>();
    const result = items.map<KanbanCard>((wo) => {
      const id = String(wo.id);
      map.set(id, wo);
      const urgencyTone = URGENCY_TONE[wo.urgency_id ?? 0];
      const badges: KanbanBadge[] = [];
      if (wo.urgency_label) {
        badges.push({
          label: wo.urgency_label,
          ...(urgencyTone ? { tone: urgencyTone } : {}),
        });
      }

      return {
        id,
        columnId: STATUS_TO_COLUMN[wo.status_code ?? "draft"] ?? "draft",
        code: wo.code,
        title: wo.title,
        ...(wo.asset_label ? { subtitle: wo.asset_label } : {}),
        ...(badges.length > 0 ? { badges } : {}),
        meta: [
          { label: wo.responsible_username ?? t("kanban.unassigned"), icon: User },
          { label: shortDate(wo.planned_end), icon: CalendarIcon },
        ],
        accent: urgencyTone ?? "neutral",
      };
    });
    byId.current = map;
    return result;
  }, [items, t]);

  const handleCardClick = useCallback(
    (card: KanbanCard) => {
      const wo = byId.current.get(card.id);
      if (wo) onCardClick(wo);
    },
    [onCardClick],
  );

  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <p className="px-1 text-xs text-text-muted" data-testid="wo-kanban-drag-hint">
        {t("kanban.statusFromDetailHint")}
      </p>
      <div className="min-h-0 flex-1">
        <KanbanBoard
          columns={columns}
          cards={cards}
          enableDrag={false}
          onCardClick={handleCardClick}
          loadMoreLabel={(count) => t("kanban.loadMore", { count })}
          emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
          aria-label={t("page.viewKanban")}
        />
      </div>
    </div>
  );
}

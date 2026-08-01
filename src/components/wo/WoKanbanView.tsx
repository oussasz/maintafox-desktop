/**
 * WoKanbanView.tsx
 *
 * Data adapter for work-order lanes. Owns status→column mapping, allowed
 * transitions, invalid-drop feedback, and open-detail — never rendering.
 */

import { Calendar as CalendarIcon, User } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { KanbanBoard } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn, KanbanTone } from "@/components/kanban";
import { pushAppToast } from "@/store/app-toast-store";
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

/**
 * Allowed drag transitions for Option B lifecycle.
 * Validation only — the detail dialog handles the actual IPC call.
 */
const ALLOWED_TRANSITIONS: Record<string, string[]> = {
  draft: ["planning", "cancelled"],
  planning: ["ready", "cancelled"],
  ready: ["in_progress", "planning", "cancelled"],
  in_progress: ["on_hold", "completed", "cancelled"],
  on_hold: ["in_progress", "cancelled"],
  completed: ["closed"],
  closed: [],
  cancelled: [],
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

  const handleDrop = useCallback(
    (card: KanbanCard, _fromColumnId: string, targetColumnId: string) => {
      const wo = byId.current.get(card.id);
      if (!wo) return;

      const fromColumn = STATUS_TO_COLUMN[wo.status_code ?? "draft"] ?? "draft";
      if (fromColumn === targetColumnId) return;

      const targetStatuses = Object.entries(STATUS_TO_COLUMN)
        .filter(([, col]) => col === targetColumnId)
        .map(([s]) => s);

      const currentStatus = wo.status_code ?? "draft";
      const validTargets = (ALLOWED_TRANSITIONS[currentStatus] ?? []).filter((to) =>
        targetStatuses.includes(to),
      );

      if (validTargets.length === 0) {
        pushAppToast({
          title: t("kanban.transitionNotAllowed", {
            code: wo.code,
            from: currentStatus.replace(/_/g, " "),
            to: targetColumnId,
          }),
          variant: "destructive",
        });
        return;
      }

      // All Option B transitions require full context — open the detail dialog.
      onCardClick(wo);
    },
    [onCardClick, t],
  );

  return (
    <KanbanBoard
      columns={columns}
      cards={cards}
      enableDrag
      onCardClick={handleCardClick}
      onDrop={handleDrop}
      loadMoreLabel={(count) => t("kanban.loadMore", { count })}
      emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
      aria-label={t("page.viewKanban")}
    />
  );
}

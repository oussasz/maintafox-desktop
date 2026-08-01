/**
 * DiKanbanBoard.tsx
 *
 * Data adapter for intervention-request lanes. Owns status→column mapping,
 * i18n, and reference labels only — all rendering belongs to KanbanBoard.
 */

import { AlertTriangle, Calendar as CalendarIcon } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { useDiReferenceLabels } from "@/components/di/di-reference-labels";
import { KanbanBoard } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn, KanbanTone } from "@/components/kanban";
import { formatShortDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

const URGENCY_TONE: Record<string, KanbanTone> = {
  critical: "critical",
  high: "high",
  medium: "medium",
  low: "low",
};

const COLUMN_DEFS = [
  { id: "submitted", labelKey: "kanban.colSubmitted", tone: "planning", statuses: ["submitted"] },
  {
    id: "pending_review",
    labelKey: "kanban.colPendingReview",
    tone: "executing",
    statuses: ["pending_review"],
  },
  {
    id: "awaiting_approval",
    labelKey: "kanban.colAwaitingApproval",
    tone: "ready",
    statuses: ["screened", "awaiting_approval"],
  },
  {
    id: "needs_clarification",
    labelKey: "kanban.colNeedsClarification",
    tone: "warning",
    statuses: ["returned_for_clarification"],
  },
  {
    id: "approved",
    labelKey: "kanban.colApproved",
    tone: "success",
    statuses: ["approved_for_planning"],
  },
  {
    id: "work",
    labelKey: "kanban.colWork",
    tone: "work",
    statuses: ["converted_to_work_order"],
  },
  {
    id: "closed",
    labelKey: "kanban.colClosed",
    tone: "muted",
    statuses: ["closed_as_non_executable", "archived"],
  },
] as const satisfies readonly {
  id: string;
  labelKey: string;
  tone: KanbanTone;
  statuses: readonly string[];
}[];

const STATUS_TO_COLUMN: Record<string, string> = Object.fromEntries(
  COLUMN_DEFS.flatMap((col) => col.statuses.map((status) => [status, col.id])),
);

interface DiKanbanBoardProps {
  items: InterventionRequest[];
  onCardClick: (di: InterventionRequest) => void;
}

export function DiKanbanBoard({ items, onCardClick }: DiKanbanBoardProps) {
  const { t, i18n } = useTranslation(["di", "common"]);
  const { originLabel } = useDiReferenceLabels();
  const locale = intlLocaleForLanguage(i18n.language);
  const byId = useRef(new Map<string, InterventionRequest>());

  const columns = useMemo<KanbanColumn[]>(
    () =>
      COLUMN_DEFS.map((col) => ({
        id: col.id,
        label: t(col.labelKey),
        tone: col.tone,
      })),
    [t],
  );

  const cards = useMemo<KanbanCard[]>(() => {
    const map = new Map<string, InterventionRequest>();
    const result: KanbanCard[] = [];

    for (const di of items) {
      const columnId = STATUS_TO_COLUMN[di.status];
      if (!columnId) continue;
      const id = String(di.id);
      map.set(id, di);

      const urgencyTone = URGENCY_TONE[di.reported_urgency];
      const badges: KanbanBadge[] = [];
      if (di.is_modified) {
        badges.push({ label: t("review.modified"), tone: "warning" });
      }
      if (urgencyTone) {
        badges.push({ label: t(`priority.${di.reported_urgency}`), tone: urgencyTone });
      }
      badges.push({ label: originLabel(di.origin_type) });
      if (di.safety_flag) {
        badges.push({ label: t("detail.safety"), tone: "danger", icon: AlertTriangle });
      }

      result.push({
        id,
        columnId,
        code: di.code,
        title: di.title,
        description: di.description,
        badges,
        link: {
          entity: "work_order",
          code: di.converted_to_wo_code,
          entityId: di.converted_to_wo_id,
          title: di.converted_to_wo_title,
        },
        meta: [{ label: formatShortDate(di.submitted_at, locale), icon: CalendarIcon }],
        accent: urgencyTone ?? "neutral",
      });
    }

    byId.current = map;
    return result;
  }, [items, locale, originLabel, t]);

  const handleCardClick = useCallback(
    (card: KanbanCard) => {
      const di = byId.current.get(card.id);
      if (di) onCardClick(di);
    },
    [onCardClick],
  );

  return (
    <KanbanBoard
      columns={columns}
      cards={cards}
      onCardClick={handleCardClick}
      emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
      aria-label={t("page.viewKanban")}
    />
  );
}

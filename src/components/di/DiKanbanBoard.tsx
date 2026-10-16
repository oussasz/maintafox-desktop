/**
 * DiKanbanBoard.tsx
 *
 * Data adapter for intervention-request lanes. Owns status→column mapping,
 * i18n, and reference labels only — all rendering belongs to KanbanBoard.
 *
 * Active operational stages only. Closed DIs live in DiArchivePanel.
 */

import { AlertTriangle, Calendar as CalendarIcon, Clock } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { useDiReferenceLabels } from "@/components/di/di-reference-labels";
import { KanbanBoard } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn } from "@/components/kanban";
import { DI_SLA_TONE, DI_STATUS_TONE, toneOf } from "@/design-system/tone";
import { formatShortDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

const COLUMN_DEFS = [
  { id: "submitted", labelKey: "kanban.colSubmitted", statuses: ["submitted"] },
  {
    id: "in_review",
    labelKey: "kanban.colPendingReview",
    statuses: ["in_review"],
  },
  {
    id: "awaiting_approval",
    labelKey: "kanban.colAwaitingApproval",
    statuses: ["awaiting_approval"],
  },
  {
    id: "needs_clarification",
    labelKey: "kanban.colNeedsClarification",
    statuses: ["returned_for_clarification"],
  },
  {
    id: "approved",
    labelKey: "kanban.colApproved",
    statuses: ["approved"],
  },
  {
    id: "deferred",
    labelKey: "kanban.colDeferred",
    statuses: ["deferred"],
  },
] as const satisfies readonly {
  id: string;
  labelKey: string;
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
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const locale = intlLocaleForLanguage(i18n.language);
  const byId = useRef(new Map<string, InterventionRequest>());

  const columns = useMemo<KanbanColumn[]>(
    () =>
      COLUMN_DEFS.map((col) => ({
        id: col.id,
        label: t(col.labelKey),
        // Lane colour = the list chip colour of the lane's status.
        tone: toneOf(DI_STATUS_TONE, col.statuses[0]),
      })),
    [t],
  );

  const cards = useMemo<KanbanCard[]>(() => {
    const map = new Map<string, InterventionRequest>();
    const result: KanbanCard[] = [];

    for (const di of items) {
      if (di.status === "closed") continue;
      const columnId = STATUS_TO_COLUMN[di.status];
      if (!columnId) continue;
      const id = String(di.id);
      map.set(id, di);

      const urgencyCode = di.validated_urgency ?? di.reported_urgency;
      const urgency = urgencyCode ? urgencyTone(urgencyCode) : undefined;
      const badges: KanbanBadge[] = [];
      if (di.sla_state === "breached" || di.sla_state === "at_risk") {
        badges.push({
          label: t(`sla.status.${di.sla_state}`),
          tone: toneOf(DI_SLA_TONE, di.sla_state),
          icon: Clock,
        });
      }
      if (urgency) {
        badges.push({ label: urgencyLabel(urgencyCode), tone: urgency });
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
        accent: urgency ?? "neutral",
      });
    }

    byId.current = map;
    return result;
  }, [items, locale, originLabel, t, urgencyLabel]);

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

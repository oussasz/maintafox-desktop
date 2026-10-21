/**
 * DiKanbanBoard.tsx
 *
 * Data adapter for intervention-request lanes. Owns status→column mapping,
 * i18n, and reference labels only — all rendering belongs to KanbanBoard.
 *
 * Active operational stages only. Closed DIs live in DiArchivePanel. Read-only: status changes
 * go through the review dialogs (see the hint rendered above the board).
 *
 * Lanes are read from the server (the list's paged call, one request per lane with the lane's
 * statuses and the list's other filters and queue): lane counts are real totals and "load more"
 * reads the lane's next page, not only the cards the list page happened to hold.
 */

import { AlertTriangle, Calendar as CalendarIcon, Clock } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { useDiReferenceLabels } from "@/components/di/di-reference-labels";
import { KanbanBoard, type ServerLaneDef, useServerColumns } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn } from "@/components/kanban";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DI_SLA_TONE, DI_STATUS_TONE, toneOf } from "@/design-system/tone";
import { listDis } from "@/services/di-service";
import { formatShortDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { DiListFilter, InterventionRequest } from "@shared/ipc-types";

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

/** Cards read per lane and per "load more". */
const LANE_PAGE_SIZE = 20;

/** Statuses of each lane, narrowed to the list's status filter when one is set. */
export function diLanes(statusFilter: readonly string[] | null | undefined): ServerLaneDef[] {
  return COLUMN_DEFS.map((col) => ({
    id: col.id,
    statuses: statusFilter?.length
      ? col.statuses.filter((s) => statusFilter.includes(s))
      : [...col.statuses],
  }));
}

interface DiKanbanBoardProps {
  /** The list's filters and queue: the lanes apply them too. */
  filter: DiListFilter;
  /** Changes when the list reloads (after a decision): the lanes are read again. */
  reloadSignal?: unknown;
  onCardClick: (di: InterventionRequest) => void;
}

export function DiKanbanBoard({ filter, reloadSignal, onCardClick }: DiKanbanBoardProps) {
  const { t, i18n } = useTranslation(["di", "common"]);
  const { originLabel } = useDiReferenceLabels();
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const locale = intlLocaleForLanguage(i18n.language);
  const byId = useRef(new Map<string, InterventionRequest>());

  // The lanes page on their own: the list's page window is not part of their query.
  const query = useMemo(() => {
    const rest: Partial<DiListFilter> = { ...filter };
    delete rest.limit;
    delete rest.offset;
    return rest;
  }, [filter]);
  const queryKey = JSON.stringify(query);
  const lanes = useMemo(() => diLanes(query.status), [queryKey]); // eslint-disable-line react-hooks/exhaustive-deps
  const fetchPage = useCallback(
    (statuses: readonly string[], offset: number, limit: number) =>
      listDis({ ...query, status: [...statuses], limit, offset }),
    [queryKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const board = useServerColumns<InterventionRequest>({
    lanes,
    fetchPage,
    pageSize: LANE_PAGE_SIZE,
    queryKey,
  });
  const items = board.items;
  const { reload } = board;
  const firstSignal = useRef(true);
  useEffect(() => {
    if (firstSignal.current) {
      firstSignal.current = false;
      return;
    }
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadSignal]);

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

  if (board.error != null) {
    return <ErrorState error={board.error} onRetry={reload} />;
  }
  if (board.loading && items.length === 0) {
    return <LoadingState />;
  }

  // Read-only board: every review step needs inputs of its own dialog (validated urgency, return
  // or deferral reason, approval checks), so a drop could only bypass them.
  return (
    <div className="flex h-full min-h-0 flex-col gap-2">
      <p className="px-1 text-xs text-text-muted" data-testid="di-kanban-status-hint">
        {t("kanban.statusFromDetailHint")}
      </p>
      <div className="min-h-0 flex-1">
        <KanbanBoard
          columns={columns}
          cards={cards}
          onCardClick={handleCardClick}
          serverPaging={{
            totals: board.totals,
            onLoadMore: board.loadMore,
            loadingMore: board.loadingMore,
          }}
          emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
          aria-label={t("page.viewKanban")}
        />
      </div>
    </div>
  );
}

/**
 * WoKanbanView.tsx
 *
 * Data adapter for work-order lanes. Owns status→column mapping and open-detail.
 *
 * Cards are not draggable: every lifecycle step has its own checks and inputs (submit,
 * readiness, permit gate on start, delay reason on hold, completion gates, cancel reason),
 * so status changes are made from the OT detail footer, not by dropping a card. The board
 * says so visibly instead of offering a drop that silently does something else.
 *
 * Lanes are read from the server (the list's paged call, one request per lane with the lane's
 * statuses and the list's other filters): lane counts are real totals and "load more" reads the
 * lane's next page, not only the cards the list page happened to hold.
 */

import { Calendar as CalendarIcon, Power, User } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { KanbanBoard, type ServerLaneDef, useServerColumns } from "@/components/kanban";
import type { KanbanBadge, KanbanCard, KanbanColumn } from "@/components/kanban";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { WO_STATUS_TONE, toneOf } from "@/design-system/tone";
import { listWos } from "@/services/wo-service";
import { formatShortDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { WoListFilter, WorkOrder } from "@shared/ipc-types";

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

/** Lanes, each coloured like the list chip of its main status (`WO_STATUS_TONE`). */
const COLUMN_META: { id: string; labelKey: string; status: string }[] = [
  { id: "draft", labelKey: "kanban.columnDraft", status: "draft" },
  { id: "planning", labelKey: "kanban.columnPlanning", status: "planning" },
  { id: "ready", labelKey: "kanban.columnReady", status: "ready" },
  { id: "executing", labelKey: "kanban.columnExecuting", status: "in_progress" },
  { id: "completed", labelKey: "kanban.columnCompleted", status: "completed" },
  { id: "closed", labelKey: "kanban.columnClosed", status: "closed" },
  { id: "cancelled", labelKey: "kanban.columnCancelled", status: "cancelled" },
];

/** Cards read per lane and per "load more". */
const LANE_PAGE_SIZE = 20;

interface WoKanbanViewProps {
  /** The list's filters (status, type, urgency, equipment, search, …): the lanes apply them too. */
  filter: WoListFilter;
  /** Changes when the list reloads (after an edit): the lanes are read again. */
  reloadSignal?: unknown;
  onCardClick: (wo: WorkOrder) => void;
}

/** Statuses of each lane, narrowed to the list's status filter when one is set. */
export function woLanes(statusFilter: readonly string[] | null | undefined): ServerLaneDef[] {
  return COLUMN_META.map((c) => {
    const statuses = Object.entries(STATUS_TO_COLUMN)
      .filter(([, col]) => col === c.id)
      .map(([status]) => status);
    return {
      id: c.id,
      statuses: statusFilter?.length ? statuses.filter((s) => statusFilter.includes(s)) : statuses,
    };
  });
}

export function WoKanbanView({ filter, reloadSignal, onCardClick }: WoKanbanViewProps) {
  const { t, i18n } = useTranslation(["ot", "common"]);
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const locale = intlLocaleForLanguage(i18n.language);
  const byId = useRef(new Map<string, WorkOrder>());

  // The lanes page on their own: the list's page window is not part of their query.
  const query = useMemo(() => {
    const rest: Partial<WoListFilter> = { ...filter };
    delete rest.limit;
    delete rest.offset;
    return rest;
  }, [filter]);
  const queryKey = JSON.stringify(query);
  const lanes = useMemo(() => woLanes(query.status_codes), [queryKey]); // eslint-disable-line react-hooks/exhaustive-deps
  const fetchPage = useCallback(
    (statuses: readonly string[], offset: number, limit: number) =>
      listWos({ ...query, status_codes: [...statuses], limit, offset }),
    [queryKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const board = useServerColumns<WorkOrder>({
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
      COLUMN_META.map((c) => ({
        id: c.id,
        label: t(c.labelKey),
        tone: toneOf(WO_STATUS_TONE, c.status),
      })),
    [t],
  );

  const cards = useMemo<KanbanCard[]>(() => {
    const map = new Map<string, WorkOrder>();
    const result = items.map<KanbanCard>((wo) => {
      const id = String(wo.id);
      map.set(id, wo);
      const urgency = wo.urgency_code ? urgencyTone(wo.urgency_code) : undefined;
      const badges: KanbanBadge[] = [];
      if (wo.urgency_code && urgency) {
        badges.push({ label: urgencyLabel(wo.urgency_code), tone: urgency });
      }
      if (wo.requires_shutdown) {
        badges.push({ label: t("shutdownRequirement.badge"), tone: "warning", icon: Power });
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
          {
            label: wo.planned_end ? formatShortDate(wo.planned_end, locale) : "—",
            icon: CalendarIcon,
          },
        ],
        accent: urgency ?? "neutral",
      };
    });
    byId.current = map;
    return result;
  }, [items, locale, t, urgencyLabel]);

  const handleCardClick = useCallback(
    (card: KanbanCard) => {
      const wo = byId.current.get(card.id);
      if (wo) onCardClick(wo);
    },
    [onCardClick],
  );

  if (board.error != null) {
    return <ErrorState error={board.error} onRetry={reload} />;
  }
  if (board.loading && items.length === 0) {
    return <LoadingState />;
  }

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
          serverPaging={{
            totals: board.totals,
            onLoadMore: board.loadMore,
            loadingMore: board.loadingMore,
          }}
          loadMoreLabel={(count) => t("kanban.loadMore", { count })}
          emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
          aria-label={t("page.viewKanban")}
        />
      </div>
    </div>
  );
}

import { ChevronDown } from "lucide-react";
import { useCallback, useMemo, useRef, useState, type DragEvent } from "react";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfKanban } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

import type {
  KanbanBadge,
  KanbanBoardProps,
  KanbanCard,
  KanbanCardAction,
  KanbanColumn,
  KanbanMetaItem,
  KanbanTone,
} from "./types";

/** Column header chrome — one class set per tone, owned here. */
const COLUMN_TONE: Record<KanbanTone, string> = {
  draft: "bg-gray-50 border-gray-200 text-gray-700",
  planning: "bg-blue-50 border-blue-200 text-blue-800",
  ready: "bg-indigo-50 border-indigo-200 text-indigo-800",
  executing: "bg-amber-50 border-amber-200 text-amber-800",
  success: "bg-teal-50 border-teal-200 text-teal-800",
  closed: "bg-neutral-50 border-neutral-200 text-neutral-600",
  cancelled: "bg-red-50 border-red-200 text-red-700",
  work: "bg-purple-50 border-purple-200 text-purple-700",
  info: "bg-blue-50 border-blue-200 text-blue-700",
  warning: "bg-amber-50 border-amber-200 text-amber-800",
  danger: "bg-red-50 border-red-200 text-red-700",
  muted: "bg-slate-50 border-slate-200 text-slate-600",
  critical: "bg-red-50 border-red-200 text-red-700",
  high: "bg-orange-50 border-orange-200 text-orange-800",
  medium: "bg-amber-50 border-amber-200 text-amber-800",
  low: "bg-emerald-50 border-emerald-200 text-emerald-800",
  neutral: "bg-slate-50 border-slate-200 text-slate-600",
};

/** Card badge chrome. */
const BADGE_TONE: Record<KanbanTone, string> = {
  draft: "bg-surface-3/70 text-text-secondary",
  planning: "bg-primary/15 text-primary",
  ready: "bg-primary/15 text-primary",
  executing: "bg-status-warning/15 text-status-warning",
  success: "bg-status-success/15 text-status-success",
  closed: "bg-surface-3/70 text-text-secondary",
  cancelled: "bg-status-danger/15 text-status-danger",
  work: "bg-primary/15 text-primary",
  info: "bg-primary/15 text-primary",
  warning: "bg-status-warning/15 text-status-warning",
  danger: "bg-status-danger/15 text-status-danger",
  muted: "bg-surface-3/70 text-text-secondary",
  critical: "bg-status-danger/15 text-status-danger",
  high: "bg-status-warning/15 text-status-warning",
  medium: "bg-status-warning/15 text-status-warning",
  low: "bg-status-success/15 text-status-success",
  neutral: "bg-surface-3/70 text-text-secondary",
};

/** Accent bar chrome — always rendered. */
const ACCENT_TONE: Record<KanbanTone, string> = {
  draft: "bg-gray-300",
  planning: "bg-blue-400",
  ready: "bg-indigo-400",
  executing: "bg-amber-400",
  success: "bg-emerald-400",
  closed: "bg-neutral-300",
  cancelled: "bg-red-600",
  work: "bg-purple-400",
  info: "bg-blue-400",
  warning: "bg-amber-400",
  danger: "bg-red-600",
  muted: "bg-gray-300",
  critical: "bg-red-600",
  high: "bg-orange-500",
  medium: "bg-amber-400",
  low: "bg-emerald-400",
  neutral: "bg-gray-300",
};

const DEFAULT_BADGE_TONE: KanbanTone = "muted";
const DEFAULT_COLUMN_TONE: KanbanTone = "muted";
const DEFAULT_EMPTY_COLUMN = "—";
const DND_MIME = "application/x-maintafox-kanban";

function CardBadge({ badge }: { badge: KanbanBadge }) {
  const Icon = badge.icon;
  return (
    <Badge
      variant="outline"
      className={cn(mfKanban.badge, BADGE_TONE[badge.tone ?? DEFAULT_BADGE_TONE])}
    >
      {Icon ? <Icon className={mfKanban.badgeIcon} aria-hidden /> : null}
      {badge.label}
    </Badge>
  );
}

function CardMetaEntry({ item }: { item: KanbanMetaItem }) {
  const Icon = item.icon;
  return (
    <span className={mfKanban.metaItem}>
      {Icon ? <Icon className={mfKanban.metaIcon} aria-hidden /> : null}
      <span className={mfKanban.metaLabel}>{item.label}</span>
    </span>
  );
}

function CardAction({ action, card }: { action: KanbanCardAction; card: KanbanCard }) {
  const Icon = action.icon;
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className={mfKanban.cardActionButton}
      disabled={action.disabled}
      onClick={(e) => {
        e.stopPropagation();
        action.onClick(card);
      }}
    >
      {Icon ? <Icon className={mfKanban.cardActionIcon} aria-hidden /> : null}
      {action.label}
    </Button>
  );
}

/**
 * Fixed card anatomy — identical for every module. Only the data differs.
 * Order: accent → code → title → subtitle → description → badges → meta → actions.
 */
function KanbanCardView({
  card,
  enableDrag,
  canDrag,
  onCardClick,
  onDragStart,
}: {
  card: KanbanCard;
  enableDrag: boolean;
  canDrag?: ((card: KanbanCard) => boolean) | undefined;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  onDragStart: (e: DragEvent, card: KanbanCard) => void;
}) {
  const isDraggable =
    enableDrag && card.draggable !== false && (canDrag ? canDrag(card) : true) && !card.disabled;

  const badges = card.badges ?? [];
  const meta = card.meta ?? [];
  const actions = card.actions ?? [];
  const hasBadgeRow = badges.length > 0 || Boolean(card.link?.code);
  const accessibleName = card.ariaLabel ?? (card.code ? `${card.code}: ${card.title}` : card.title);

  return (
    <div
      role="button"
      tabIndex={card.disabled ? -1 : 0}
      aria-label={accessibleName}
      aria-disabled={card.disabled || undefined}
      draggable={isDraggable}
      className={cn(
        mfKanban.card,
        isDraggable && mfKanban.cardDraggable,
        card.selected && mfKanban.cardSelected,
        card.disabled && mfKanban.cardDisabled,
      )}
      onClick={() => {
        if (!card.disabled) onCardClick?.(card);
      }}
      onKeyDown={(e) => {
        if (card.disabled) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onCardClick?.(card);
        }
      }}
      onDragStart={(e) => {
        if (!isDraggable) {
          e.preventDefault();
          return;
        }
        onDragStart(e, card);
      }}
    >
      <div className={cn(mfKanban.accentBar, ACCENT_TONE[card.accent ?? "neutral"])} aria-hidden />

      <div className={mfKanban.cardBody}>
        {card.code ? (
          <div className={mfKanban.cardHeader}>
            <span className={mfKanban.cardCode}>{card.code}</span>
          </div>
        ) : null}

        <p className={mfKanban.cardTitle}>{card.title}</p>

        {card.subtitle ? <p className={mfKanban.cardSubtitle}>{card.subtitle}</p> : null}

        {card.description ? <p className={mfKanban.cardDescription}>{card.description}</p> : null}

        {hasBadgeRow ? (
          <div className={mfKanban.badgeRow}>
            {badges.map((badge, i) => (
              <CardBadge key={`${badge.label}-${i}`} badge={badge} />
            ))}
            {card.link ? (
              <LinkedEntityBadge
                entity={card.link.entity}
                code={card.link.code}
                entityId={card.link.entityId}
                title={card.link.title}
                className={mfKanban.linkBadge}
              />
            ) : null}
          </div>
        ) : null}

        {meta.length > 0 ? (
          <div className={mfKanban.cardMeta}>
            {meta.map((item, i) => (
              <CardMetaEntry key={`${item.label}-${i}`} item={item} />
            ))}
          </div>
        ) : null}

        {actions.length > 0 ? (
          <div className={mfKanban.cardActions}>
            {actions.map((action) => (
              <CardAction key={action.id} action={action} card={card} />
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function KanbanColumnView({
  column,
  cards,
  enableDrag,
  canDrag,
  onCardClick,
  onDragStart,
  onColumnDrop,
  pageSize,
  loadMoreLabel,
  emptyColumnLabel,
}: {
  column: KanbanColumn;
  cards: KanbanCard[];
  enableDrag: boolean;
  canDrag?: ((card: KanbanCard) => boolean) | undefined;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  onDragStart: (e: DragEvent, card: KanbanCard) => void;
  onColumnDrop: (e: DragEvent, toColumnId: string) => void;
  pageSize: number;
  loadMoreLabel?: ((remaining: number) => string) | undefined;
  emptyColumnLabel: string;
}) {
  const [visibleCount, setVisibleCount] = useState(pageSize);
  const [dragOver, setDragOver] = useState(false);

  const visible = cards.slice(0, visibleCount);
  const remaining = cards.length - visibleCount;

  return (
    <div
      className={cn(mfKanban.column, dragOver && mfKanban.columnDragOver)}
      data-column-id={column.id}
      onDragOver={(e) => {
        if (!enableDrag) return;
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={() => setDragOver(false)}
      onDrop={(e) => {
        if (!enableDrag) return;
        setDragOver(false);
        onColumnDrop(e, column.id);
      }}
    >
      <div className={cn(mfKanban.columnHeader, COLUMN_TONE[column.tone ?? DEFAULT_COLUMN_TONE])}>
        <div className={mfKanban.columnHeaderLabel}>
          <span className={mfKanban.columnTitle}>{column.label}</span>
        </div>
        <Badge variant="outline" className={mfKanban.countBadge}>
          {cards.length}
        </Badge>
      </div>

      <div className={mfKanban.columnBody} role="list">
        {visible.length === 0 ? (
          <div className={mfKanban.columnEmpty}>{emptyColumnLabel}</div>
        ) : (
          visible.map((card) => (
            <div key={card.id} role="listitem">
              <KanbanCardView
                card={card}
                enableDrag={enableDrag}
                {...(canDrag ? { canDrag } : {})}
                {...(onCardClick ? { onCardClick } : {})}
                onDragStart={onDragStart}
              />
            </div>
          ))
        )}
      </div>

      {remaining > 0 ? (
        <div className={mfKanban.loadMore}>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className={mfKanban.loadMoreButton}
            onClick={() => setVisibleCount((n) => n + pageSize)}
          >
            <ChevronDown className="h-3.5 w-3.5" aria-hidden />
            {loadMoreLabel ? loadMoreLabel(remaining) : String(remaining)}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

/**
 * The single Kanban rendering engine for the whole application.
 *
 * Adapters supply columns, cards, metadata, actions, and callbacks only.
 * All chrome — headers, counters, cards, badges, spacing, typography, empty
 * states, colours, borders, shadows — lives here so every board is identical.
 */
export function KanbanBoard<T = KanbanCard>({
  columns,
  cards: cardsProp,
  items,
  getCard,
  onCardClick,
  enableDrag = false,
  canDrag,
  canDrop,
  onDrop,
  pageSize = 20,
  loadMoreLabel,
  emptyColumnLabel = DEFAULT_EMPTY_COLUMN,
  empty,
  className,
  "aria-label": ariaLabel = "Kanban board",
}: KanbanBoardProps<T>) {
  const dragCardRef = useRef<KanbanCard | null>(null);

  const cards = useMemo(() => {
    if (cardsProp) return cardsProp;
    if (items && getCard) return items.map((item, i) => getCard(item, i));
    return [];
  }, [cardsProp, items, getCard]);

  const byColumn = useMemo(() => {
    const map = new Map<string, KanbanCard[]>(columns.map((c) => [c.id, []]));
    for (const card of cards) {
      map.get(card.columnId)?.push(card);
    }
    return map;
  }, [columns, cards]);

  const handleDragStart = useCallback((e: DragEvent, card: KanbanCard) => {
    dragCardRef.current = card;
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData(
      DND_MIME,
      JSON.stringify({ cardId: card.id, fromColumnId: card.columnId }),
    );
    e.dataTransfer.setData("text/plain", card.id);
  }, []);

  const handleColumnDrop = useCallback(
    (e: DragEvent, toColumnId: string) => {
      e.preventDefault();
      const card = dragCardRef.current;
      dragCardRef.current = null;
      if (!card) return;
      if (card.columnId === toColumnId) return;
      if (canDrop && !canDrop(card, card.columnId, toColumnId)) return;
      onDrop?.(card, card.columnId, toColumnId);
    },
    [canDrop, onDrop],
  );

  if (columns.length === 0) {
    return (
      <div className={cn(mfKanban.root, className)} role="region" aria-label={ariaLabel}>
        <div className={mfKanban.boardEmpty}>{empty}</div>
      </div>
    );
  }

  return (
    <div className={cn(mfKanban.root, className)} role="region" aria-label={ariaLabel}>
      <div className={mfKanban.scroller}>
        {columns.map((column) => (
          <KanbanColumnView
            key={column.id}
            column={column}
            cards={byColumn.get(column.id) ?? []}
            enableDrag={enableDrag}
            {...(canDrag ? { canDrag } : {})}
            {...(onCardClick ? { onCardClick } : {})}
            onDragStart={handleDragStart}
            onColumnDrop={handleColumnDrop}
            pageSize={pageSize}
            emptyColumnLabel={emptyColumnLabel}
            {...(loadMoreLabel ? { loadMoreLabel } : {})}
          />
        ))}
      </div>
    </div>
  );
}

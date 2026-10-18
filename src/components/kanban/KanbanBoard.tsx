import { ArrowRightLeft, ChevronDown } from "lucide-react";
import {
  type DragEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { StatusChip } from "@/components/ui/status-chip";
import { mfKanban } from "@/design-system/tokens";
import { mfTone } from "@/design-system/tone";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";

import type {
  KanbanBadge,
  KanbanBoardProps,
  KanbanCard,
  KanbanCardAction,
  KanbanColumn,
  KanbanMetaItem,
  KanbanMoveOutcome,
  KanbanServerPaging,
  KanbanTone,
} from "./types";

/**
 * Colours are the seven design-system tones (`design-system/tone.ts`) — the same classes as
 * the list chips, light and dark. Unset tones are neutral (grey); colour is only used for a
 * meaning the adapter passes (status, urgency, SLA, safety).
 */
const DEFAULT_TONE: KanbanTone = "neutral";
const DND_MIME = "application/x-maintafox-kanban";

/** Normalizes an `onDrop` outcome: accepted, or refused with an optional localized reason. */
function readOutcome(outcome: KanbanMoveOutcome): { ok: boolean; reason?: string | undefined } {
  if (outcome === undefined || outcome === true) return { ok: true };
  if (outcome === false) return { ok: false };
  return outcome;
}

function cardName(card: KanbanCard): string {
  return card.ariaLabel ?? (card.code ? `${card.code}: ${card.title}` : card.title);
}

function CardBadge({ badge }: { badge: KanbanBadge }) {
  return (
    <StatusChip
      label={badge.label}
      tone={badge.tone ?? DEFAULT_TONE}
      {...(badge.icon ? { icon: badge.icon } : {})}
    />
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

/** "Move to" menu — the pointer-free, screen-reader-friendly alternative to dragging. */
function CardMoveMenu({
  card,
  targets,
  onMove,
}: {
  card: KanbanCard;
  targets: KanbanColumn[];
  onMove: (card: KanbanCard, toColumnId: string) => Promise<void>;
}) {
  const { t } = useTranslation("common");
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={mfKanban.cardActionButton}
          aria-label={t("kanban.moveCard", { card: cardName(card) })}
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <ArrowRightLeft className={mfKanban.cardActionIcon} aria-hidden />
          {t("kanban.move")}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" onClick={(e) => e.stopPropagation()}>
        <DropdownMenuLabel>{t("kanban.moveTo")}</DropdownMenuLabel>
        {targets.map((column) => (
          <DropdownMenuItem key={column.id} onSelect={() => void onMove(card, column.id)}>
            <span
              className={cn(mfKanban.columnDot, mfTone[column.tone ?? DEFAULT_TONE].dot)}
              aria-hidden
            />
            {column.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Fixed card anatomy — identical for every module. Only the data differs.
 * Order: accent → code → title → subtitle → description → badges → meta → actions.
 */
function KanbanCardView({
  card,
  movable,
  moveTargets,
  moveHintId,
  onCardClick,
  onDragStart,
  onMove,
  onKeyboardMove,
}: {
  card: KanbanCard;
  movable: boolean;
  moveTargets: KanbanColumn[];
  moveHintId: string;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  onDragStart: (e: DragEvent, card: KanbanCard) => void;
  onMove: (card: KanbanCard, toColumnId: string) => Promise<void>;
  onKeyboardMove: (card: KanbanCard, direction: -1 | 1) => void;
}) {
  const badges = card.badges ?? [];
  const meta = card.meta ?? [];
  const actions = card.actions ?? [];
  const hasBadgeRow = badges.length > 0 || Boolean(card.link?.code);
  const showMoveMenu = movable && moveTargets.length > 0;

  return (
    <div
      role="button"
      tabIndex={card.disabled ? -1 : 0}
      aria-label={cardName(card)}
      aria-disabled={card.disabled || undefined}
      aria-describedby={movable ? moveHintId : undefined}
      data-card-id={card.id}
      draggable={movable}
      className={cn(
        mfKanban.card,
        movable && mfKanban.cardDraggable,
        card.selected && mfKanban.cardSelected,
        card.disabled && mfKanban.cardDisabled,
      )}
      onClick={() => {
        if (!card.disabled) onCardClick?.(card);
      }}
      onKeyDown={(e: KeyboardEvent<HTMLDivElement>) => {
        if (card.disabled || e.target !== e.currentTarget) return;
        if (movable && e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
          e.preventDefault();
          onKeyboardMove(card, e.key === "ArrowLeft" ? -1 : 1);
          return;
        }
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onCardClick?.(card);
        }
      }}
      onDragStart={(e) => {
        if (!movable) {
          e.preventDefault();
          return;
        }
        onDragStart(e, card);
      }}
    >
      <div
        className={cn(mfKanban.accentBar, mfTone[card.accent ?? DEFAULT_TONE].bar)}
        aria-hidden
      />

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

        {actions.length > 0 || showMoveMenu ? (
          <div className={mfKanban.cardActions}>
            {actions.map((action) => (
              <CardAction key={action.id} action={action} card={card} />
            ))}
            {showMoveMenu ? (
              <CardMoveMenu card={card} targets={moveTargets} onMove={onMove} />
            ) : null}
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
  isMovable,
  moveTargetsOf,
  moveHintId,
  onCardClick,
  onDragStart,
  onColumnDrop,
  onMove,
  onKeyboardMove,
  pageSize,
  serverPaging,
  loadMoreLabel,
  emptyColumnLabel,
}: {
  column: KanbanColumn;
  cards: KanbanCard[];
  enableDrag: boolean;
  isMovable: (card: KanbanCard) => boolean;
  moveTargetsOf: (card: KanbanCard) => KanbanColumn[];
  moveHintId: string;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  onDragStart: (e: DragEvent, card: KanbanCard) => void;
  onColumnDrop: (e: DragEvent, toColumnId: string) => void;
  onMove: (card: KanbanCard, toColumnId: string) => Promise<void>;
  onKeyboardMove: (card: KanbanCard, direction: -1 | 1) => void;
  pageSize: number;
  serverPaging?: KanbanServerPaging | undefined;
  loadMoreLabel: (remaining: number) => string;
  emptyColumnLabel: string;
}) {
  const [visibleCount, setVisibleCount] = useState(pageSize);
  const [dragOver, setDragOver] = useState(false);
  const titleId = useId();

  // Server-paged lanes show every card they have read and count the server total; otherwise the
  // lane pages through the cards it was given.
  const serverTotal = serverPaging ? (serverPaging.totals[column.id] ?? cards.length) : null;
  const visible = serverPaging ? cards : cards.slice(0, visibleCount);
  const remaining = serverTotal != null ? serverTotal - cards.length : cards.length - visibleCount;
  const loadingMore = serverPaging?.loadingMore?.[column.id] ?? false;

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
      <div className={mfKanban.columnHeader}>
        <div className={mfKanban.columnHeaderLabel}>
          <span
            className={cn(mfKanban.columnDot, mfTone[column.tone ?? DEFAULT_TONE].dot)}
            aria-hidden
          />
          <span id={titleId} className={mfKanban.columnTitle}>
            {column.label}
          </span>
        </div>
        <Badge variant="outline" className={mfKanban.countBadge}>
          {serverTotal ?? cards.length}
        </Badge>
      </div>

      <div className={mfKanban.columnBody} role="list" aria-labelledby={titleId}>
        {visible.length === 0 ? (
          <div className={mfKanban.columnEmpty}>{emptyColumnLabel}</div>
        ) : (
          visible.map((card) => (
            <div key={card.id} role="listitem">
              <KanbanCardView
                card={card}
                movable={isMovable(card)}
                moveTargets={isMovable(card) ? moveTargetsOf(card) : []}
                moveHintId={moveHintId}
                {...(onCardClick ? { onCardClick } : {})}
                onDragStart={onDragStart}
                onMove={onMove}
                onKeyboardMove={onKeyboardMove}
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
            disabled={loadingMore}
            onClick={() =>
              serverPaging
                ? serverPaging.onLoadMore(column.id)
                : setVisibleCount((n) => n + pageSize)
            }
          >
            <ChevronDown className="h-3.5 w-3.5" aria-hidden />
            {loadMoreLabel(remaining)}
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
 *
 * Moving a card (when `enableDrag`): drag and drop, the card's "Move to" menu, or Alt+←/→ on
 * the focused card. All three go through the same `canDrop` → `onDrop` path; the result is
 * announced to screen readers and focus stays on the moved card.
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
  serverPaging,
  loadMoreLabel,
  emptyColumnLabel,
  empty,
  className,
  "aria-label": ariaLabel,
}: KanbanBoardProps<T>) {
  const { t } = useTranslation("common");
  const dragCardRef = useRef<KanbanCard | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  /** Card to focus in its new lane once the server accepted the move (set only on success). */
  const [focusAfterMove, setFocusAfterMove] = useState<{ cardId: string; columnId: string } | null>(
    null,
  );
  const [announcement, setAnnouncement] = useState("");
  const moveHintId = useId();

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

  const isMovable = useCallback(
    (card: KanbanCard) =>
      enableDrag && card.draggable !== false && !card.disabled && (canDrag ? canDrag(card) : true),
    [enableDrag, canDrag],
  );

  const accepts = useCallback(
    (card: KanbanCard, toColumnId: string) =>
      card.columnId !== toColumnId && (!canDrop || canDrop(card, card.columnId, toColumnId)),
    [canDrop],
  );

  const moveTargetsOf = useCallback(
    (card: KanbanCard) => columns.filter((column) => accepts(card, column.id)),
    [columns, accepts],
  );

  /**
   * One path for drag, menu and keyboard: run the adapter's transition, then announce the real
   * outcome. Focus follows the card only once the server accepted the move.
   */
  const moveCard = useCallback(
    async (card: KanbanCard, toColumnId: string) => {
      if (!accepts(card, toColumnId) || !onDrop) return;
      const target = columns.find((c) => c.id === toColumnId);
      const name = cardName(card);
      try {
        const outcome = readOutcome(await onDrop(card, card.columnId, toColumnId));
        if (outcome.ok) {
          setFocusAfterMove({ cardId: card.id, columnId: toColumnId });
          setAnnouncement(t("kanban.moved", { card: name, column: target?.label ?? "" }));
        } else {
          setFocusAfterMove(null);
          setAnnouncement(
            outcome.reason
              ? t("kanban.refusedReason", { card: name, reason: outcome.reason })
              : t("kanban.refused", { card: name }),
          );
        }
      } catch (err) {
        setFocusAfterMove(null);
        setAnnouncement(t("kanban.refusedReason", { card: name, reason: appErrorReason(err) }));
      }
    },
    [accepts, columns, onDrop, t],
  );

  const keyboardMove = useCallback(
    (card: KanbanCard, direction: -1 | 1) => {
      const from = columns.findIndex((c) => c.id === card.columnId);
      for (let i = from + direction; i >= 0 && i < columns.length; i += direction) {
        const column = columns[i];
        if (column && accepts(card, column.id)) {
          void moveCard(card, column.id);
          return;
        }
      }
      setAnnouncement(t("kanban.noTarget", { card: cardName(card) }));
    },
    [columns, accepts, moveCard, t],
  );

  // Keep keyboard users on the card they moved once the adapter re-renders it in its new lane.
  // Waits for the card to appear in the target lane (the adapter may reload after or before the
  // transition resolves), then focuses it once.
  useEffect(() => {
    if (!focusAfterMove || !rootRef.current) return;
    const el = rootRef.current.querySelector<HTMLElement>(
      `[data-column-id="${CSS.escape(focusAfterMove.columnId)}"] [data-card-id="${CSS.escape(
        focusAfterMove.cardId,
      )}"]`,
    );
    if (!el) return;
    if (document.activeElement !== el) el.focus();
    setFocusAfterMove(null);
  }, [cards, focusAfterMove]);

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
      void moveCard(card, toColumnId);
    },
    [moveCard],
  );

  const boardLabel = ariaLabel ?? t("kanban.boardLabel");
  const columnEmpty = emptyColumnLabel ?? t("kanban.emptyColumn");
  const moreLabel = loadMoreLabel ?? ((count: number) => t("kanban.showMore", { count }));

  if (columns.length === 0) {
    return (
      <div className={cn(mfKanban.root, className)} role="region" aria-label={boardLabel}>
        <div className={mfKanban.boardEmpty}>{empty}</div>
      </div>
    );
  }

  return (
    <div
      ref={rootRef}
      className={cn(mfKanban.root, className)}
      role="region"
      aria-label={boardLabel}
    >
      {enableDrag ? (
        <>
          <span id={moveHintId} className="sr-only">
            {t("kanban.moveHint")}
          </span>
          <span className="sr-only" role="status" aria-live="polite">
            {announcement}
          </span>
        </>
      ) : null}
      <div className={mfKanban.scroller}>
        {columns.map((column) => (
          <KanbanColumnView
            key={column.id}
            column={column}
            cards={byColumn.get(column.id) ?? []}
            enableDrag={enableDrag}
            isMovable={isMovable}
            moveTargetsOf={moveTargetsOf}
            moveHintId={moveHintId}
            {...(onCardClick ? { onCardClick } : {})}
            onDragStart={handleDragStart}
            onColumnDrop={handleColumnDrop}
            onMove={moveCard}
            onKeyboardMove={keyboardMove}
            pageSize={pageSize}
            {...(serverPaging ? { serverPaging } : {})}
            emptyColumnLabel={columnEmpty}
            loadMoreLabel={moreLabel}
          />
        ))}
      </div>
    </div>
  );
}

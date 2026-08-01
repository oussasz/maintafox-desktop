import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { LinkedEntityKind } from "@/components/common/LinkedEntityBadge";

/**
 * Single semantic tone vocabulary for every board surface (column header,
 * badge, accent bar). KanbanBoard owns the class mapping — adapters never
 * pass colours, spacing, typography, or any other Tailwind for board chrome.
 */
export type KanbanTone =
  | "draft"
  | "planning"
  | "ready"
  | "executing"
  | "success"
  | "closed"
  | "cancelled"
  | "work"
  | "info"
  | "warning"
  | "danger"
  | "muted"
  | "critical"
  | "high"
  | "medium"
  | "low"
  | "neutral";

export interface KanbanBadge {
  label: string;
  tone?: KanbanTone | undefined;
  icon?: LucideIcon | undefined;
}

/** Icon + text pair rendered in the card footer meta row. */
export interface KanbanMetaItem {
  label: string;
  icon?: LucideIcon | undefined;
}

export interface KanbanCardAction {
  id: string;
  label: string;
  icon?: LucideIcon | undefined;
  onClick: (card: KanbanCard) => void;
  disabled?: boolean | undefined;
}

/** First-class business link (DI -> WO, …). Data only; board renders the badge. */
export interface KanbanCardLink {
  entity: LinkedEntityKind;
  code: string | null | undefined;
  entityId?: number | null | undefined;
  title?: string | null | undefined;
}

export interface KanbanColumn {
  id: string;
  /** Already-localized column label. */
  label: string;
  tone?: KanbanTone | undefined;
}

/**
 * Pure presentation data for one card. Fixed anatomy (board-owned):
 * accent → code → title → subtitle → description → badges → meta → actions.
 * Missing optional fields omit the node without changing sibling roles.
 */
export interface KanbanCard {
  id: string;
  columnId: string;
  /** Business code (never an internal id) rendered in the code row. */
  code?: string | undefined;
  title: string;
  /** Secondary identity line (asset, supplier, source). */
  subtitle?: string | undefined;
  /** Longer text; the board clamps it. */
  description?: string | undefined;
  /** Priority / status / origin / safety chips — fixed badge row. */
  badges?: KanbanBadge[] | undefined;
  link?: KanbanCardLink | undefined;
  /** Dates / people — fixed meta row with shared icon+label chrome. */
  meta?: KanbanMetaItem[] | undefined;
  /** Accent bar tone (urgency / priority). Defaults to neutral. */
  accent?: KanbanTone | undefined;
  actions?: KanbanCardAction[] | undefined;
  /** When false, card is not HTML5-draggable even if the board enables drag. */
  draggable?: boolean | undefined;
  disabled?: boolean | undefined;
  selected?: boolean | undefined;
  /** Accessible name override; defaults to code + title. */
  ariaLabel?: string | undefined;
}

export interface KanbanBoardProps<T = KanbanCard> {
  columns: KanbanColumn[];
  /** Pre-mapped cards, or use items + getCard. */
  cards?: KanbanCard[] | undefined;
  items?: T[] | undefined;
  getCard?: ((item: T, index: number) => KanbanCard) | undefined;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  enableDrag?: boolean | undefined;
  canDrag?: ((card: KanbanCard) => boolean) | undefined;
  canDrop?: ((card: KanbanCard, fromColumnId: string, toColumnId: string) => boolean) | undefined;
  /**
   * Called after a drop when canDrop allows (or when canDrop is omitted).
   * Adapters own validation feedback and open-detail side effects.
   */
  onDrop?: ((card: KanbanCard, fromColumnId: string, toColumnId: string) => void) | undefined;
  pageSize?: number | undefined;
  /** Already-localized load-more text builder. */
  loadMoreLabel?: ((remaining: number) => string) | undefined;
  /**
   * Shared empty-column label for every column (from common.kanban.emptyColumn).
   * Adapters must not pass per-column empty copy.
   */
  emptyColumnLabel?: string | undefined;
  /** Board-level empty text when there are no columns. */
  empty?: ReactNode | undefined;
  className?: string | undefined;
  "aria-label"?: string | undefined;
}

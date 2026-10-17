import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { LinkedEntityKind } from "@/components/common/LinkedEntityBadge";
import type { Tone } from "@/design-system/tone";

/**
 * Board colours are the seven status tones of the design system (`design-system/tone.ts`), the
 * same vocabulary as the list chips: adapters pass `toneOf(WO_STATUS_TONE, code)`,
 * `urgencyTone(code)` … so a status or an urgency level has the same colour on the board and
 * in the list. KanbanBoard owns the class mapping — adapters never pass Tailwind classes.
 */
export type KanbanTone = Tone;

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
  /**
   * When false, the card cannot be moved (neither dragged nor moved with the keyboard / the
   * "Move to" menu) even if the board enables moving.
   */
  draggable?: boolean | undefined;
  disabled?: boolean | undefined;
  selected?: boolean | undefined;
  /** Accessible name override; defaults to code + title. */
  ariaLabel?: string | undefined;
}

/**
 * Outcome of a move, returned (or resolved) by `onDrop`:
 * - `undefined` / `true` / `{ ok: true }` — the server accepted the transition;
 * - `false` / `{ ok: false, reason }` — refused (reason already localized; a generic refusal is
 *   announced without one);
 * - a thrown error / rejected promise — failed; announced with the shared error text.
 */
export type KanbanMoveOutcome = void | boolean | { ok: boolean; reason?: string | undefined };

export interface KanbanBoardProps<T = KanbanCard> {
  columns: KanbanColumn[];
  /** Pre-mapped cards, or use items + getCard. */
  cards?: KanbanCard[] | undefined;
  items?: T[] | undefined;
  getCard?: ((item: T, index: number) => KanbanCard) | undefined;
  onCardClick?: ((card: KanbanCard) => void) | undefined;
  /**
   * Cards can change column: by drag and drop, by the card's "Move to" menu, or with
   * Alt+←/→ on a focused card (keyboard alternative to dragging).
   */
  enableDrag?: boolean | undefined;
  canDrag?: ((card: KanbanCard) => boolean) | undefined;
  canDrop?: ((card: KanbanCard, fromColumnId: string, toColumnId: string) => boolean) | undefined;
  /**
   * Called after a drop / keyboard move when canDrop allows (or when canDrop is omitted). It must
   * run the module's own server-side transition command; its outcome is announced to screen
   * readers ("moved" only once accepted). Adapters own toasts and open-detail side effects.
   */
  onDrop?:
    | ((
        card: KanbanCard,
        fromColumnId: string,
        toColumnId: string,
      ) => KanbanMoveOutcome | Promise<KanbanMoveOutcome>)
    | undefined;
  pageSize?: number | undefined;
  /** Already-localized load-more text builder (defaults to common.kanban.showMore). */
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

/**
 * Shared layout tokens for the Reference Manager domain tree.
 * Pure Flexbox — no JS measuring. Label always wins remaining width.
 */

/** Domain / set row shell: hover + selected + focus chrome stay on this class. */
export const DOMAIN_TREE_ROW_CLASS =
  "group flex min-w-0 items-center gap-2 px-3 py-1.5 text-sm cursor-pointer hover:bg-surface-1";

export const DOMAIN_TREE_ROW_SELECTED_CLASS = "bg-surface-2 font-medium text-text-primary";

export const DOMAIN_TREE_ROW_IDLE_CLASS = "text-text-secondary";

export const DOMAIN_TREE_ROW_FOCUS_CLASS = "ring-1 ring-inset ring-primary/40";

/** Fixed chevron column — expand/collapse only. */
export const DOMAIN_TREE_CHEVRON_SLOT =
  "flex h-7 w-5 shrink-0 items-center justify-center text-text-muted";

/** Folder / leading glyph — never shrinks. */
export const DOMAIN_TREE_LEADING_ICON = "h-4 w-4 shrink-0 text-text-muted";

/**
 * Primary label region. Takes all leftover space; truncates only when necessary.
 * Sibling badges/actions must be `shrink-0` so they cannot starve this flex item.
 */
export const DOMAIN_TREE_LABEL_CLASS =
  "min-w-0 flex-1 truncate text-left leading-5";

/**
 * Governance badge slot — equal width on every row so labels align vertically.
 * Content truncates inside the slot; full name goes on `title`.
 */
export const DOMAIN_TREE_BADGE_SLOT =
  "inline-flex h-5 w-[5.25rem] shrink-0 items-center justify-center overflow-hidden rounded border border-surface-border bg-transparent px-1 text-[10px] font-normal leading-none text-text-muted";

export const DOMAIN_TREE_BADGE_TEXT = "block w-full truncate text-center";

/** Trailing action rail (lock + overflow). Fixed so lock stays on the right axis. */
export const DOMAIN_TREE_TRAIL_CLASS = "flex shrink-0 items-center gap-0.5";

export const DOMAIN_TREE_ICON_SLOT =
  "flex h-7 w-7 shrink-0 items-center justify-center [&_button]:shrink-0";

/** Child set indent aligns under domain label (chevron + gap + folder + gap). */
export const DOMAIN_TREE_SET_INDENT = "pl-10 pr-3";

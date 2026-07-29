/**
 * Shared UI tokens for Reference Manager tables (RVE + synthetic adapters).
 * Keeps action column buttons and chrome visually identical across domains.
 */

import { cn } from "@/lib/utils";

/** Empty actions-column placeholder shared by real + synthetic tables. */
export const REF_TABLE_EMPTY_ACTIONS_MARK = "—";

/**
 * Native `title` reason when a destructive control or active switch is disabled.
 * Priority: system > inactive > missing permission.
 */
export function refTableDisabledActionTitle(opts: {
  isSystem?: boolean;
  isInactive?: boolean;
  missingPermission?: boolean;
  t: (key: string) => unknown;
}): string | undefined {
  if (opts.isSystem) return String(opts.t("editor.disabledReason.systemValue"));
  if (opts.isInactive) return String(opts.t("editor.disabledReason.alreadyInactive"));
  if (opts.missingPermission) return String(opts.t("editor.disabledReason.noPermission"));
  return undefined;
}

/** Ghost icon button used for edit / delete / confirm / cancel in reference tables. */
export const REF_TABLE_ICON_BTN_CLASS =
  "h-6 w-6 shrink-0 hover:bg-muted/90 focus-visible:ring-1 focus-visible:ring-ring";

export function refTableIconButtonClass(extra?: string): string {
  return cn(REF_TABLE_ICON_BTN_CLASS, extra);
}

/** Actions column: identical layout for all reference tables (CAD-grade alignment). */
export const REF_TABLE_ACTIONS_GROUP_CLASS = "flex items-center justify-end gap-2";

/** Header "add" button token used by all reference manager panels. */
export const REF_TABLE_HEADER_ADD_BTN_CLASS = "gap-1.5 min-w-[128px] justify-center";

export function refTableHeaderAddButtonClass(extra?: string): string {
  return cn(REF_TABLE_HEADER_ADD_BTN_CLASS, extra);
}

/** Root flex column for ReferenceValueTable. */
export const REF_TABLE_ROOT_CLASS = "flex h-full flex-col";

/** Scroll region wrapping the grid. */
export const REF_TABLE_SCROLL_CLASS = "flex-1 overflow-auto";

/** Standard header cell padding / typography. */
export const REF_TABLE_HEADER_CELL_CLASS = "px-3 py-2 font-medium text-text-muted";

/** Standard body cell — fixed row rhythm across domains. */
export const REF_TABLE_BODY_CELL_CLASS = "px-3 py-1.5";

/** Standard data row chrome. */
export const REF_TABLE_ROW_CLASS = "border-b border-surface-border hover:bg-surface-1";

/** Title typography in table header. */
export const REF_TABLE_TITLE_CLASS = "truncate text-sm font-semibold text-text-primary";

/** Compact badge used for status / category chips. */
export const REF_TABLE_BADGE_CLASS = "text-[10px]";

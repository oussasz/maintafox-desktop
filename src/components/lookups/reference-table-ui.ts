/**
 * Shared UI tokens for Reference Manager tables (TVA / WO / inventory panels).
 * Keeps action column buttons visually identical across domains.
 */

import { cn } from "@/lib/utils";

/** Ghost icon button used for edit / delete / confirm / cancel in reference tables. */
export const REF_TABLE_ICON_BTN_CLASS =
  "h-6 w-6 shrink-0 hover:bg-muted/90 focus-visible:ring-1 focus-visible:ring-ring";

export function refTableIconButtonClass(extra?: string): string {
  return cn(REF_TABLE_ICON_BTN_CLASS, extra);
}

/** Actions column: identical layout for all reference tables (CAD-grade alignment). */
export const REF_TABLE_ACTIONS_GROUP_CLASS = "flex items-center justify-end gap-2";

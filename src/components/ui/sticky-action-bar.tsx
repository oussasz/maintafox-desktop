/**
 * StickyActionBar — the main actions of a long form or page, pinned to the bottom of the
 * scrolling area so the key button is always in view (no scrolling down to find "Enregistrer").
 *
 * Put it as the LAST child of the element that scrolls (a dialog body, a long page). `hint` is
 * the sentence that says why the main action is unavailable ("Pour créer : titre, équipement"):
 * a disabled button never stays silent.
 */
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export interface StickyActionBarProps {
  children: ReactNode;
  /** Why the main action is disabled, or a short reminder; shown above the buttons. */
  hint?: ReactNode;
  className?: string;
}

export function StickyActionBar({ children, hint, className }: StickyActionBarProps) {
  return (
    <div
      className={cn(
        "sticky bottom-0 z-10 -mx-1 border-t border-surface-border bg-surface-1 px-1 pb-1 pt-3",
        className,
      )}
    >
      {hint ? (
        <p role="status" className="mb-2 text-right text-xs text-text-muted">
          {hint}
        </p>
      ) : null}
      <div className="flex justify-end gap-2">{children}</div>
    </div>
  );
}

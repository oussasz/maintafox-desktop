/**
 * ActionReason — the visible sentence that says why a main action (Créer, Enregistrer, Étudier…)
 * is unavailable. Same markup as the `hint` of StickyActionBar, for places where that bar does not
 * fit (dialog footers, page headers, toolbars). Unlike a `title` tooltip it is readable with the
 * keyboard and on touch screens. Renders nothing when there is no reason (the action is available).
 *
 * Name the MISSING CONDITION in plain words ("Cochez au moins un équipement"), never a code.
 */
import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export interface ActionReasonProps {
  children?: ReactNode;
  className?: string;
  /** Lets the button point at the sentence with `aria-describedby`. */
  id?: string;
  "data-testid"?: string;
}

export function ActionReason({ children, className, ...rest }: ActionReasonProps) {
  if (!children) return null;
  return (
    <p role="status" className={cn("text-xs text-text-muted", className)} {...rest}>
      {children}
    </p>
  );
}

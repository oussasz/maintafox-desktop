import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type EmptyStateProps = {
  title: string;
  description?: string;
  icon?: ReactNode;
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
  className?: string;
};

/**
 * Shared empty-state for list modules. Optional primary CTA mirrors the page-header create action.
 */
export function EmptyState({
  title,
  description,
  icon,
  actionLabel,
  onAction,
  actionDisabled,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-2 rounded-md border border-dashed px-6 py-10 text-center",
        className,
      )}
    >
      {icon ? <div className="text-muted-foreground mb-1">{icon}</div> : null}
      <p className="text-sm font-medium text-foreground">{title}</p>
      {description ? <p className="text-muted-foreground max-w-sm text-xs">{description}</p> : null}
      {actionLabel && onAction ? (
        <Button size="sm" className="mt-2" onClick={onAction} disabled={actionDisabled}>
          {actionLabel}
        </Button>
      ) : null}
    </div>
  );
}

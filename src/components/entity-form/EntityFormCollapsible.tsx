import { ChevronDown } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormCollapsibleProps {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  /** Default true — advanced blocks start collapsed. */
  defaultClosed?: boolean;
  className?: string;
}

export function EntityFormCollapsible({
  title,
  description,
  children,
  defaultClosed = true,
  className,
}: EntityFormCollapsibleProps) {
  const [open, setOpen] = useState(!defaultClosed);
  const panelId = useId();

  return (
    <div className={cn("space-y-2", className)}>
      <button
        type="button"
        className={mfEntityForm.collapsibleTrigger}
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="min-w-0 flex-1">
          <span className="block">{title}</span>
          {description != null && (
            <span className="mt-0.5 block text-xs font-normal text-text-muted">{description}</span>
          )}
        </span>
        <ChevronDown
          className={cn(
            "h-4 w-4 shrink-0 text-text-muted transition-transform",
            open && "rotate-180",
          )}
          aria-hidden
        />
      </button>
      {open && (
        <div id={panelId} className="space-y-3 px-0.5 pb-1">
          {children}
        </div>
      )}
    </div>
  );
}

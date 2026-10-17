import { CheckCircle2, Circle } from "lucide-react";

import { mfAuthAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

type StatusItem = {
  id: string;
  label: string;
  done: boolean;
};

type ActivationStatusCardProps = {
  items: StatusItem[];
  waitingLabel?: string;
};

/** Checklist of activation checks. Turns green once every check has passed. */
export function ActivationStatusCard({ items, waitingLabel }: ActivationStatusCardProps) {
  const allDone = items.length > 0 && items.every((item) => item.done);
  return (
    <div
      className={cn(allDone ? mfAuthAlert.success : mfAuthAlert.info, "space-y-2")}
      role="status"
    >
      <ul className="space-y-1.5">
        {items.map((item) => (
          <li key={item.id} className="flex items-center gap-2 text-sm font-medium">
            {item.done ? (
              <CheckCircle2 className="h-4 w-4 shrink-0 text-[var(--mfa-success)]" aria-hidden />
            ) : (
              <Circle className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
            )}
            <span>{item.label}</span>
          </li>
        ))}
      </ul>
      {waitingLabel ? <p className="pl-6 text-xs text-text-secondary">{waitingLabel}</p> : null}
    </div>
  );
}

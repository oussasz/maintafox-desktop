import { mfAlert } from "@/design-system/tokens";
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

export function ActivationStatusCard({ items, waitingLabel }: ActivationStatusCardProps) {
  return (
    <div className={cn(mfAlert.info, "space-y-2")} role="status">
      <ul className="space-y-1.5">
        {items.map((item) => (
          <li key={item.id} className="flex items-center gap-2 text-sm">
            <span aria-hidden>{item.done ? "✓" : "○"}</span>
            <span>{item.label}</span>
          </li>
        ))}
      </ul>
      {waitingLabel ? <p className="text-xs text-text-secondary">{waitingLabel}</p> : null}
    </div>
  );
}

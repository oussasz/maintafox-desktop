import { cn } from "@/lib/utils";

export type ActivationStepId = "validate" | "activate" | "finish";

const STEP_ORDER: ActivationStepId[] = ["validate", "activate", "finish"];

type ActivationStepperProps = {
  current: ActivationStepId;
  labels: Record<ActivationStepId, string>;
};

export function ActivationStepper({ current, labels }: ActivationStepperProps) {
  const currentIndex = STEP_ORDER.indexOf(current);

  return (
    <ol className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between" aria-label="Activation progress">
      {STEP_ORDER.map((id, index) => {
        const done = index < currentIndex;
        const active = index === currentIndex;
        return (
          <li
            key={id}
            className={cn(
              "flex min-w-0 items-center gap-2 text-sm",
              done && "text-status-success",
              active && "font-medium text-text-primary",
              !done && !active && "text-text-muted",
            )}
            aria-current={active ? "step" : undefined}
          >
            <span
              className={cn(
                "flex h-6 w-6 shrink-0 items-center justify-center rounded-full border text-xs",
                done && "border-status-success bg-status-success/15",
                active && "border-primary bg-primary/10 text-primary",
                !done && !active && "border-surface-border",
              )}
              aria-hidden
            >
              {done ? "✓" : active ? "●" : "○"}
            </span>
            <span className="truncate">{labels[id]}</span>
          </li>
        );
      })}
    </ol>
  );
}

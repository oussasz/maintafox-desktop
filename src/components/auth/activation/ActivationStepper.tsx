import { Check } from "lucide-react";

import { cn } from "@/lib/utils";

export type ActivationStepId = "validate" | "activate" | "finish";

const STEP_ORDER: ActivationStepId[] = ["validate", "activate", "finish"];

type ActivationStepperProps = {
  current: ActivationStepId;
  labels: Record<ActivationStepId, string>;
  /** Compact summary shown on narrow widths, e.g. "Step 2 of 3". */
  progressLabel?: string;
};

/**
 * Three-step progress. From the `sm` breakpoint: numbered nodes joined by a line, labels below.
 * Below it: a one-line "Step n of 3 · current step" summary with a segmented bar. The ordered
 * list stays available to assistive technology at every width.
 */
export function ActivationStepper({ current, labels, progressLabel }: ActivationStepperProps) {
  const currentIndex = STEP_ORDER.indexOf(current);
  const total = STEP_ORDER.length;

  return (
    <div>
      <div className="sm:hidden" aria-hidden>
        <p className="text-xs font-medium text-text-secondary">
          <span className="font-semibold text-text-primary">
            {progressLabel ?? `${currentIndex + 1}/${total}`}
          </span>
          {" · "}
          {labels[current]}
        </p>
        <div className="mt-2 flex gap-1.5">
          {STEP_ORDER.map((id, index) => (
            <span
              key={id}
              className={cn(
                "h-1 flex-1 rounded-full",
                index <= currentIndex ? "bg-[var(--mfa-blue)]" : "bg-[var(--mfa-border)]",
              )}
            />
          ))}
        </div>
      </div>

      <ol className="flex max-sm:sr-only sm:items-start" aria-label="Activation progress">
        {STEP_ORDER.map((id, index) => {
          const done = index < currentIndex;
          const active = index === currentIndex;
          return (
            <li
              key={id}
              className="relative flex min-w-0 flex-1 flex-col items-center text-center"
              aria-current={active ? "step" : undefined}
            >
              {index < total - 1 ? (
                <span
                  aria-hidden
                  className={cn(
                    "absolute left-1/2 top-3.5 h-0.5 w-full -translate-y-1/2",
                    done ? "bg-[var(--mfa-blue)]" : "bg-[var(--mfa-border)]",
                  )}
                />
              ) : null}
              <span
                aria-hidden
                className={cn(
                  "relative z-10 flex h-7 w-7 items-center justify-center rounded-full border-2 text-xs font-semibold transition-colors",
                  done && "border-[var(--mfa-blue)] bg-[var(--mfa-blue)] text-white",
                  active &&
                    "border-[var(--mfa-blue)] bg-white text-[var(--mfa-blue)] ring-4 ring-[var(--mfa-blue)]/15",
                  !done && !active && "border-[var(--mfa-border-strong)] bg-white text-text-muted",
                )}
              >
                {done ? <Check className="h-3.5 w-3.5" strokeWidth={3} /> : index + 1}
              </span>
              <span
                className={cn(
                  "mt-2 max-w-full truncate px-1 text-xs",
                  active ? "font-semibold text-text-primary" : "font-medium",
                  done && "text-text-secondary",
                  !done && !active && "text-text-muted",
                )}
              >
                {labels[id]}
              </span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

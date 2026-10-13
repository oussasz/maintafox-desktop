import { forwardRef, useId, type InputHTMLAttributes, type ReactNode } from "react";

import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface AuthFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "id"> {
  id: string;
  label: string;
  /** Leading icon (decorative). */
  icon?: ReactNode;
  /** Trailing control inside the field, e.g. show/hide password or paste. Needs its own label. */
  trailing?: ReactNode;
  /** Help text under the field. */
  hint?: string;
  /** `warning` renders the hint in the attention colour (e.g. Caps Lock is on). */
  hintTone?: "muted" | "warning";
  /** Validation message; sets `aria-invalid` and is announced politely. */
  error?: string | null;
  /** Extra classes on the input element. */
  inputClassName?: string;
  /** Extra classes on the wrapper. */
  className?: string;
}

/**
 * Labelled input for authentication forms: 44 px tall, brand-blue focus ring, accessible
 * description wiring (`aria-describedby`) for hint and error text.
 */
export const AuthField = forwardRef<HTMLInputElement, AuthFieldProps>(function AuthField(
  {
    id,
    label,
    icon,
    trailing,
    hint,
    hintTone = "muted",
    error,
    inputClassName,
    className,
    ...inputProps
  },
  ref,
) {
  const reactId = useId();
  const hintId = hint ? `${id}-hint-${reactId}` : undefined;
  const errorId = error ? `${id}-error-${reactId}` : undefined;
  const describedBy = [errorId, hintId].filter(Boolean).join(" ") || undefined;

  return (
    <div className={className}>
      <label htmlFor={id} className={mfInput.authLabel}>
        {label}
      </label>
      <div className="relative">
        {icon ? (
          <span
            className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-muted [&>svg]:h-4 [&>svg]:w-4"
            aria-hidden
          >
            {icon}
          </span>
        ) : null}
        <input
          ref={ref}
          id={id}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={cn(
            trailing ? mfInput.authFieldWithTrailing : mfInput.authField,
            !icon && "pl-3",
            inputClassName,
          )}
          {...inputProps}
        />
        {trailing ? (
          <div className="absolute inset-y-0 right-1.5 flex items-center">{trailing}</div>
        ) : null}
      </div>
      {error ? (
        <p id={errorId} role="status" className="mt-1.5 text-[13px] text-[var(--mfa-danger)]">
          {error}
        </p>
      ) : null}
      {hint ? (
        <p
          id={hintId}
          className={cn(
            "mt-1.5 text-[13px]",
            hintTone === "warning" ? "font-medium text-[var(--text-warning)]" : "text-text-muted",
          )}
        >
          {hint}
        </p>
      ) : null}
    </div>
  );
});

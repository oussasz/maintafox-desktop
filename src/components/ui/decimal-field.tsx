/**
 * Entry field for a quantity, an amount or a duration.
 *
 * Shared by every screen so a number is typed and shown the same way everywhere: a text field
 * (a native number field follows the Windows language, not the app's), "2,5" and "2.5" both
 * accepted, the app language's separator shown back, no exponent, no spinner step.
 * The parent keeps a `number | null`; what the user is typing stays in the field until the
 * field is left, so a half-typed "2," is never rewritten under the cursor.
 */
import * as React from "react";

import { Input, type InputProps } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";

export interface DecimalFieldProps extends Omit<
  InputProps,
  "value" | "defaultValue" | "onChange" | "type" | "inputMode"
> {
  value: number | null;
  /** Called with the number typed, or null when the field is emptied. Not called while the text is not a number. */
  onValueChange: (value: number | null) => void;
  /** Most decimal places (default 2: amounts; use 3 for quantities). */
  decimals?: number;
  /** Always show `decimals` places (amounts); default trims trailing zeros. */
  fixed?: boolean;
  allowNegative?: boolean;
  min?: number;
  max?: number;
}

export const DecimalField = React.forwardRef<HTMLInputElement, DecimalFieldProps>(
  (
    {
      value,
      onValueChange,
      decimals = 2,
      fixed = false,
      allowNegative = false,
      min,
      max,
      className,
      onBlur,
      "aria-invalid": ariaInvalid,
      ...props
    },
    ref,
  ) => {
    const { formatDecimalInput, parseDecimal } = useFormatters();
    const show = React.useCallback(
      (v: number | null) => formatDecimalInput(v, { decimals, fixed }),
      [formatDecimalInput, decimals, fixed],
    );
    const [draft, setDraft] = React.useState(() => show(value));
    const [invalid, setInvalid] = React.useState(false);
    const last = React.useRef<number | null>(value);

    // The value changed from outside (a reset, a loaded record): show it. Typing that already
    // produced this value leaves the draft alone.
    React.useEffect(() => {
      if (value !== last.current) {
        last.current = value;
        setDraft(show(value));
        setInvalid(false);
      }
    }, [value, show]);

    const outOfRange = (n: number) => (min != null && n < min) || (max != null && n > max);

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      const text = e.target.value;
      setDraft(text);
      const parsed = parseDecimal(text, { allowNegative });
      if (parsed.kind === "invalid") {
        setInvalid(true);
        return;
      }
      if (parsed.kind === "empty") {
        setInvalid(false);
        last.current = null;
        onValueChange(null);
        return;
      }
      setInvalid(outOfRange(parsed.value));
      last.current = parsed.value;
      onValueChange(parsed.value);
    };

    const handleBlur = (e: React.FocusEvent<HTMLInputElement>) => {
      const parsed = parseDecimal(draft, { allowNegative });
      if (parsed.kind === "number") {
        const factor = 10 ** decimals;
        const rounded = Math.round(parsed.value * factor) / factor;
        if (rounded !== parsed.value) {
          last.current = rounded;
          onValueChange(rounded);
        }
        setDraft(show(rounded));
      } else if (parsed.kind === "empty") {
        setDraft("");
      }
      onBlur?.(e);
    };

    return (
      <Input
        {...props}
        ref={ref}
        inputMode="decimal"
        autoComplete="off"
        value={draft}
        onChange={handleChange}
        onBlur={handleBlur}
        aria-invalid={ariaInvalid ?? (invalid || undefined)}
        className={cn("tabular-nums", invalid && "border-status-danger", className)}
      />
    );
  },
);
DecimalField.displayName = "DecimalField";

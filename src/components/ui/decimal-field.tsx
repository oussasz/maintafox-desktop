/**
 * Entry field for a quantity, an amount or a duration.
 *
 * Shared by every screen so a number is typed and shown the same way everywhere: a text field
 * (a native number field follows the Windows language, not the app's), "2,5" and "2.5" both
 * accepted, the app language's separator shown back, no exponent, no spinner step.
 * The parent keeps a `number | null`; what the user is typing stays in the field until the
 * field is left, so a half-typed "2," is never rewritten under the cursor.
 *
 * The point is always the decimal mark ("1.250" is 1,25, never one thousand two hundred fifty);
 * a space is the only thousands separator accepted.
 *
 * While the text typed is not a number (or is out of range) the field is invalid: red border,
 * `aria-invalid`, `onValidityChange(true)` for the parent, and Enter does nothing (no implicit
 * form submit, the field's own `onKeyDown` is not called) so the parent's previous value is never
 * submitted behind text the user can still see.
 */
import * as React from "react";

import { Input, type InputProps } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { roundDecimal } from "@/utils/decimal";

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
  /** Called with true while the text typed is not an acceptable number, false once it is. */
  onValidityChange?: (invalid: boolean) => void;
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
      onFocus,
      onKeyDown,
      onValidityChange,
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
    const focused = React.useRef(false);

    // The value changed from outside (a reset, a loaded record): show it. Typing that already
    // produced this value leaves the draft alone.
    React.useEffect(() => {
      // While the user is typing, the field is theirs: a parent that turns an emptied field into
      // 0 must not put "0" back under the cursor. The blur shows what the parent holds.
      if (focused.current) return;
      if (value !== last.current) {
        last.current = value;
        setDraft(show(value));
        setInvalid(false);
      }
    }, [value, show]);

    const validityRef = React.useRef(onValidityChange);
    validityRef.current = onValidityChange;
    const reported = React.useRef(false);
    React.useEffect(() => {
      if (reported.current === invalid) return;
      reported.current = invalid;
      validityRef.current?.(invalid);
    }, [invalid]);

    const outOfRange = (n: number) => (min != null && n < min) || (max != null && n > max);

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      const text = e.target.value;
      setDraft(text);
      // A lone minus sign is the start of a negative number, not an error yet.
      if (allowNegative && text.trim() === "-") {
        setInvalid(false);
        return;
      }
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

    // Enter with text that is not an acceptable number must not submit the previous value.
    const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") {
        const parsed = parseDecimal(draft, { allowNegative });
        const blocked =
          invalid ||
          parsed.kind === "invalid" ||
          (allowNegative && draft.trim() === "-") ||
          (parsed.kind === "number" && outOfRange(parsed.value));
        if (blocked) {
          e.preventDefault();
          e.stopPropagation();
          setInvalid(true);
          return;
        }
      }
      onKeyDown?.(e);
    };

    const handleFocus = (e: React.FocusEvent<HTMLInputElement>) => {
      focused.current = true;
      onFocus?.(e);
    };

    const handleBlur = (e: React.FocusEvent<HTMLInputElement>) => {
      focused.current = false;
      if (value !== last.current) {
        // The parent holds something other than what was typed (an emptied field it keeps as 0,
        // or a change made from outside while the field had focus): show what it holds.
        last.current = value;
        setDraft(show(value));
        setInvalid(false);
        onBlur?.(e);
        return;
      }
      const parsed = parseDecimal(draft, { allowNegative });
      if (parsed.kind === "number") {
        const rounded = roundDecimal(parsed.value, decimals);
        if (rounded !== parsed.value) {
          last.current = rounded;
          onValueChange(rounded);
        }
        setDraft(show(rounded));
      } else {
        // Left empty, or with text that is not a number: the field shows what the parent holds,
        // so what is on screen is what will be saved.
        last.current = value;
        setDraft(parsed.kind === "empty" ? "" : show(value));
        setInvalid(false);
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
        onFocus={handleFocus}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        aria-invalid={ariaInvalid ?? (invalid || undefined)}
        className={cn("tabular-nums", invalid && "border-status-danger", className)}
      />
    );
  },
);
DecimalField.displayName = "DecimalField";

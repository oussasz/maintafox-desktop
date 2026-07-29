import * as React from "react";

import { Input, type InputProps } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export type TimeInputProps = Omit<InputProps, "type" | "step" | "value" | "onChange"> & {
  /** Controlled value in `HH:mm` (24h). Empty string when unset. */
  value: string;
  onChange: (value: string) => void;
};

/**
 * Enterprise time control — `HH:mm` only via native `type="time"`.
 * Never use free-text inputs for schedule measures.
 */
export const TimeInput = React.forwardRef<HTMLInputElement, TimeInputProps>(
  ({ className, value, onChange, disabled, ...props }, ref) => {
    return (
      <Input
        ref={ref}
        type="time"
        step={60}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
        className={cn("h-9 font-mono tabular-nums", className)}
        {...props}
      />
    );
  },
);
TimeInput.displayName = "TimeInput";

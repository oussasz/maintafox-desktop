import type { LucideIcon } from "lucide-react";

import { type Tone, mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

export interface StatusChipProps {
  /** Translated label — never a raw status code. */
  label: string;
  tone: Tone;
  icon?: LucideIcon;
  /** Leading dot instead of an icon (dense tables). */
  dot?: boolean;
  size?: "sm" | "md";
  title?: string;
  className?: string;
}

/**
 * The one status/priority chip of the app. Colour comes from the tone vocabulary
 * (`design-system/tone.ts`), so a given meaning looks the same in every module.
 */
export function StatusChip({
  label,
  tone,
  icon: Icon,
  dot = false,
  size = "sm",
  title,
  className,
}: StatusChipProps) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex max-w-full items-center gap-1 whitespace-nowrap rounded-full border font-medium",
        size === "sm" ? "px-2 py-0.5 text-[11px]" : "px-2.5 py-1 text-xs",
        mfTone[tone].chip,
        className,
      )}
    >
      {Icon ? <Icon className="h-3 w-3 shrink-0" aria-hidden /> : null}
      {!Icon && dot ? (
        <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", mfTone[tone].dot)} aria-hidden />
      ) : null}
      <span className="truncate">{label}</span>
    </span>
  );
}

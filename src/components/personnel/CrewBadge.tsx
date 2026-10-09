/**
 * CrewBadge — Lucide crest in circular accent badge, or 2-letter monogram fallback.
 */

import {
  CREW_COLOR_SWATCHES,
  crewMonogram,
  resolveCrewIcon,
} from "@/components/personnel/crew-crest";
import { cn } from "@/lib/utils";

export type CrewBadgeProps = {
  code: string;
  name: string;
  iconKey?: string | null;
  accentColor?: string | null;
  size?: "sm" | "md" | "lg";
  className?: string;
};

const SIZE_CLASS = {
  sm: "h-7 w-7 text-[10px]",
  md: "h-9 w-9 text-xs",
  lg: "h-11 w-11 text-sm",
} as const;

const ICON_SIZE = {
  sm: "h-3.5 w-3.5",
  md: "h-4 w-4",
  lg: "h-5 w-5",
} as const;

export function CrewBadge({
  code,
  name,
  iconKey,
  accentColor,
  size = "md",
  className,
}: CrewBadgeProps) {
  const Icon = resolveCrewIcon(iconKey);
  const trimmedAccent = typeof accentColor === "string" ? accentColor.trim() : "";
  const accent = /^#[0-9a-fA-F]{6}$/.test(trimmedAccent) ? trimmedAccent : null;
  const useIcon = Icon != null && accent != null;
  const monogram = crewMonogram(code, name);
  const muted = CREW_COLOR_SWATCHES[10] ?? "#64748b";

  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-full border font-semibold tracking-wide",
        SIZE_CLASS[size],
        className,
      )}
      style={
        useIcon || accent
          ? {
              backgroundColor: accent ?? muted,
              borderColor: accent ?? muted,
              color: "#fff",
            }
          : {
              backgroundColor: "hsl(var(--muted, 210 20% 96%))",
              borderColor: "hsl(var(--surface-border, 214 20% 88%))",
              color: "hsl(var(--text-muted, 215 16% 47%))",
            }
      }
      title={name}
      aria-hidden
    >
      {useIcon && Icon ? <Icon className={ICON_SIZE[size]} strokeWidth={2.25} /> : monogram}
    </span>
  );
}

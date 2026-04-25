/**
 * Industrial-style color preview: rounded square swatch + uppercase hex (reference tables).
 */

import { cn } from "@/lib/utils";

export interface ReferenceColorSwatchHexProps {
  color: string | null | undefined;
  /** Default `sm` = 16px swatch; `md` = 20px for primary color columns. */
  size?: "sm" | "md";
  className?: string;
}

function formatHexDisplay(raw: string): string {
  const t = raw.trim();
  if (!t) return "—";
  return t.startsWith("#") ? t.toUpperCase() : t;
}

export function ReferenceColorSwatchHex({
  color,
  size = "sm",
  className,
}: ReferenceColorSwatchHexProps) {
  const raw = (color ?? "").trim();
  const swatchClass = size === "md" ? "h-5 w-5" : "h-4 w-4";

  if (!raw) {
    return <span className={cn("text-sm text-text-muted tabular-nums", className)}>—</span>;
  }

  return (
    <div className={cn("flex items-center gap-2 align-middle", className)}>
      <span
        className={cn(
          "inline-block shrink-0 rounded-md border border-surface-border bg-surface-2",
          swatchClass,
        )}
        style={{ backgroundColor: raw }}
        aria-hidden
      />
      <span className="font-mono text-sm text-text-primary tabular-nums tracking-tight">
        {formatHexDisplay(raw)}
      </span>
    </div>
  );
}

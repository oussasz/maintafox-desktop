import { logoColor } from "@/assets/logo";
import { cn } from "@/lib/utils";

export type MaintafoxWordmarkSize = "sm" | "md" | "lg" | "xl";

export interface MaintafoxWordmarkProps {
  className?: string;
  size?: MaintafoxWordmarkSize;
  /** Show “Maintafox” next to the mark */
  showText?: boolean;
  /** Horizontal alignment of the row */
  align?: "start" | "center";
  /** Visual tone: shell uses theme tokens; auth uses brand blue on light cards */
  tone?: "shell" | "auth";
  /** Extra classes on the word text */
  textClassName?: string;
}

const SIZE: Record<MaintafoxWordmarkSize, { img: string; text: string }> = {
  sm: { img: "h-6 w-6", text: "text-sm" },
  md: { img: "h-7 w-7", text: "text-base" },
  lg: { img: "h-9 w-9", text: "text-xl" },
  xl: { img: "h-11 w-11", text: "text-2xl" },
};

/**
 * Product logo + wordmark for shell header, auth, activation, and other standalone surfaces.
 * Uses bundled SVG so it works reliably in the Tauri WebView without relying on public URL paths.
 */
export function MaintafoxWordmark({
  className,
  size = "md",
  showText = true,
  align = "start",
  tone = "shell",
  textClassName,
}: MaintafoxWordmarkProps) {
  const s = SIZE[size];
  const textTone =
    tone === "auth" ? "font-bold text-primary-dark" : "font-semibold text-text-primary";

  return (
    <div
      className={cn(
        "flex items-center gap-2.5 select-none",
        align === "center" && "justify-center",
        className,
      )}
    >
      <img
        src={logoColor}
        alt={showText ? "" : "Maintafox"}
        decoding="async"
        className={cn(s.img, "shrink-0 object-contain object-left")}
        aria-hidden={showText}
      />
      {showText ? (
        <span className={cn("tracking-tight", s.text, textTone, textClassName)}>Maintafox</span>
      ) : null}
    </div>
  );
}

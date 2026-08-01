import type { ReactElement } from "react";

/** Shared stroke for schematic-style sharp outlines (high contrast via currentColor). */
const SW = 1.65;

/**
 * IEC-style AND — flat vertical back, semicircular front (D-shape).
 * viewBox 0 0 48 40 — out at right bulge (~34,20), ins at (~8,14)(~8,26).
 */
export function IconAndGate({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 40" className={className} aria-hidden preserveAspectRatio="xMidYMid meet">
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth={SW}
        strokeLinejoin="miter"
        vectorEffect="non-scaling-stroke"
        d="M8 8L22 8A12 12 0 0 1 34 20A12 12 0 0 1 22 32L8 32L8 8z"
      />
      <path
        stroke="currentColor"
        strokeWidth={SW}
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
        d="M2 14h6M2 26h6"
      />
    </svg>
  );
}

/**
 * IEC OR — curved input side (shield), pointed output + horizontal stem.
 */
export function IconOrGate({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 40" className={className} aria-hidden preserveAspectRatio="xMidYMid meet">
      <path
        fill="none"
        stroke="currentColor"
        strokeWidth={SW}
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
        d="M4 12H10M4 28H10M10 8Q22 8 28 20Q22 32 10 32M10 8V32M28 20H44"
      />
    </svg>
  );
}

/**
 * Basic event — circle + output stem on top (IEC-style leaf / annotation pin up).
 * viewBox 0 0 48 48 — stem exits at (24,4), circle cx=24 cy=28 r=11.
 */
export function IconBasicEvent({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 48" className={className} aria-hidden preserveAspectRatio="xMidYMid meet">
      <path
        stroke="currentColor"
        strokeWidth={SW}
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
        d="M24 4L24 19"
      />
      <circle
        cx="24"
        cy="30"
        r="11"
        fill="none"
        stroke="currentColor"
        strokeWidth={SW}
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

/** Palette / non-block previews only — functional BLK uses RamsFlowNodes layout. */
export function IconRbdBlock({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 40" className={className} aria-hidden>
      <rect
        x="8"
        y="10"
        width="32"
        height="20"
        rx="1"
        fill="none"
        stroke="currentColor"
        strokeWidth={SW}
        vectorEffect="non-scaling-stroke"
      />
      <path
        stroke="currentColor"
        strokeWidth={SW}
        vectorEffect="non-scaling-stroke"
        d="M2 20h6M40 20h6"
      />
    </svg>
  );
}

export function IconRbdSeries({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 40" className={className} aria-hidden preserveAspectRatio="xMidYMid meet">
      <rect
        x="4"
        y="12"
        width="12"
        height="16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
      <rect
        x="18"
        y="12"
        width="12"
        height="16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
      <rect
        x="32"
        y="12"
        width="12"
        height="16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
      <path
        stroke="currentColor"
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
        d="M2 20h2M46 20h-2"
      />
    </svg>
  );
}

export function IconRbdParallel({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 48 40" className={className} aria-hidden preserveAspectRatio="xMidYMid meet">
      <path
        stroke="currentColor"
        strokeWidth={SW}
        fill="none"
        vectorEffect="non-scaling-stroke"
        d="M8 8v24M8 20h8l4-8v16l4-8h8"
      />
      <path
        stroke="currentColor"
        strokeWidth={SW}
        vectorEffect="non-scaling-stroke"
        d="M2 14h6M2 26h6M40 14h6M40 26h6"
      />
    </svg>
  );
}

export type RamsLabToolIcon = (p: { className?: string }) => ReactElement;

export const RAMS_FTA_TOOL_ICONS: Record<"ftaAnd" | "ftaOr" | "ftaBe", RamsLabToolIcon> = {
  ftaAnd: IconAndGate,
  ftaOr: IconOrGate,
  ftaBe: IconBasicEvent,
};

export const RAMS_RBD_TOOL_ICONS: Record<
  "rbdBlock" | "rbdSeries" | "rbdParallel",
  RamsLabToolIcon
> = {
  rbdBlock: IconRbdBlock,
  rbdSeries: IconRbdSeries,
  rbdParallel: IconRbdParallel,
};

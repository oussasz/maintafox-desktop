/**
 * ResourceTimeline semantic color map — SSOT for resource-vs-time views.
 * Adapters must use these tokens; do not invent one-off Tailwind hex in modules.
 * CSS custom properties live in resource-timeline.css (theme-aware).
 */

export type ResourceTimelineState =
  | "free"
  | "partial"
  | "blocked"
  | "critical"
  | "rest"
  | "shift"
  | "commitment"
  | "leave"
  | "medical"
  | "restriction"
  | "training";

/** Block types drawn as chronology bars (legend for ResourceTimeline items). */
export const RESOURCE_TIMELINE_BLOCK_LEGEND_STATES = [
  "leave",
  "medical",
  "restriction",
  "training",
] as const satisfies ReadonlyArray<ResourceTimelineState>;

/** Horizon length presets (inclusive day counts). Single config — no magic numbers in UI. */
export const RESOURCE_TIMELINE_HORIZON_PRESETS = [7, 14, 28] as const;
export type ResourceTimelineHorizonPreset = (typeof RESOURCE_TIMELINE_HORIZON_PRESETS)[number];
export const RESOURCE_TIMELINE_DEFAULT_HORIZON_DAYS: ResourceTimelineHorizonPreset = 7;

/** Density — consumed by ResourceTimeline primitive (override via props when needed). */
export const RESOURCE_TIMELINE_SIDEBAR_WIDTH = 220;
/** Fits two-line sidebar cells (name + code) without clipping; must match canvas group row height. */
export const RESOURCE_TIMELINE_LINE_HEIGHT = 52;
export const RESOURCE_TIMELINE_ITEM_HEIGHT_RATIO = 0.7;
/** Fallback when ResizeObserver has not measured yet (hidden tab / zero height). */
export const RESOURCE_TIMELINE_MIN_HEIGHT = 240;

/** CSS custom properties defined in resource-timeline.css */
export const RESOURCE_TIMELINE_STATE_VAR: Record<ResourceTimelineState, string> = {
  free: "var(--rt-state-free)",
  partial: "var(--rt-state-partial)",
  blocked: "var(--rt-state-blocked)",
  critical: "var(--rt-state-critical)",
  rest: "var(--rt-state-rest)",
  shift: "var(--rt-state-shift)",
  commitment: "var(--rt-state-commitment)",
  leave: "var(--rt-state-leave)",
  medical: "var(--rt-state-medical)",
  restriction: "var(--rt-state-restriction)",
  training: "var(--rt-state-training)",
};

export const RESOURCE_TIMELINE_STATE_BORDER_VAR: Record<ResourceTimelineState, string> = {
  free: "var(--rt-state-free-border)",
  partial: "var(--rt-state-partial-border)",
  blocked: "var(--rt-state-blocked-border)",
  critical: "var(--rt-state-critical-border)",
  rest: "var(--rt-state-rest-border)",
  shift: "var(--rt-state-shift-border)",
  commitment: "var(--rt-state-commitment-border)",
  leave: "var(--rt-state-leave-border)",
  medical: "var(--rt-state-medical-border)",
  restriction: "var(--rt-state-restriction-border)",
  training: "var(--rt-state-training-border)",
};

export const RESOURCE_TIMELINE_STATE_FG_VAR: Record<ResourceTimelineState, string> = {
  free: "var(--rt-state-free-fg)",
  partial: "var(--rt-state-partial-fg)",
  blocked: "var(--rt-state-blocked-fg)",
  critical: "var(--rt-state-critical-fg)",
  rest: "var(--rt-state-rest-fg)",
  shift: "var(--rt-state-shift-fg)",
  commitment: "var(--rt-state-commitment-fg)",
  leave: "var(--rt-state-leave-fg)",
  medical: "var(--rt-state-medical-fg)",
  restriction: "var(--rt-state-restriction-fg)",
  training: "var(--rt-state-training-fg)",
};

/** Tailwind utility classes for legends / heatmap cells (token-aligned, not arbitrary hex) */
export const RESOURCE_TIMELINE_STATE_CLASS: Record<ResourceTimelineState, string> = {
  free: "bg-[var(--rt-state-free)] text-[var(--rt-state-free-fg)] border-[var(--rt-state-free-border)]",
  partial:
    "bg-[var(--rt-state-partial)] text-[var(--rt-state-partial-fg)] border-[var(--rt-state-partial-border)]",
  blocked:
    "bg-[var(--rt-state-blocked)] text-[var(--rt-state-blocked-fg)] border-[var(--rt-state-blocked-border)]",
  critical:
    "bg-[var(--rt-state-critical)] text-[var(--rt-state-critical-fg)] border-[var(--rt-state-critical-border)]",
  rest: "bg-[var(--rt-state-rest)] text-[var(--rt-state-rest-fg)] border-[var(--rt-state-rest-border)]",
  shift:
    "bg-[var(--rt-state-shift)] text-[var(--rt-state-shift-fg)] border-[var(--rt-state-shift-border)]",
  commitment:
    "bg-[var(--rt-state-commitment)] text-[var(--rt-state-commitment-fg)] border-[var(--rt-state-commitment-border)]",
  leave:
    "bg-[var(--rt-state-leave)] text-[var(--rt-state-leave-fg)] border-[var(--rt-state-leave-border)]",
  medical:
    "bg-[var(--rt-state-medical)] text-[var(--rt-state-medical-fg)] border-[var(--rt-state-medical-border)]",
  restriction:
    "bg-[var(--rt-state-restriction)] text-[var(--rt-state-restriction-fg)] border-[var(--rt-state-restriction-border)]",
  training:
    "bg-[var(--rt-state-training)] text-[var(--rt-state-training-fg)] border-[var(--rt-state-training-border)]",
};

/** Item class suffix for CSS skin (mf-rt-item--{state}). */
export function resourceTimelineItemStateClass(state: ResourceTimelineState): string {
  return `mf-rt-item mf-rt-item--${state}`;
}

export function blockTypeToTimelineState(
  blockType: string,
  isCritical: boolean,
): ResourceTimelineState {
  if (isCritical || blockType === "medical" || blockType === "restriction") {
    if (blockType === "medical") return "medical";
    if (blockType === "restriction") return "restriction";
    return "critical";
  }
  if (blockType === "leave") return "leave";
  if (blockType === "training") return "training";
  return "blocked";
}

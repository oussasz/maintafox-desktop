/**
 * Status tones — the single colour vocabulary for state across Maintafox.
 *
 * Every business status (OT, DI, PM occurrence, permit, PO, inspection round …) maps to ONE
 * of these tones, so the same meaning always has the same colour in every module:
 *
 * | tone      | meaning (examples)                                          |
 * |-----------|-------------------------------------------------------------|
 * | neutral   | not started / archived (draft, closed, deferred)            |
 * | info      | queued, waiting for someone else (submitted, planning)      |
 * | progress  | work is happening (in progress, in review, released)        |
 * | attention | needs a decision from the reader (awaiting approval, hold)  |
 * | warning   | at risk (due soon, low stock, clarification requested)      |
 * | success   | done and accepted (approved, completed, verified)           |
 * | danger    | failed, overdue, blocked, cancelled, safety                 |
 *
 * Colours come from `--tone-*` CSS variables (light + dark in globals.css); each foreground is
 * ≥ 4.5:1 on its background. Never use raw Tailwind palette colours (`bg-amber-100`) for state.
 */

export type Tone = "neutral" | "info" | "progress" | "attention" | "warning" | "success" | "danger";

export const TONES: readonly Tone[] = [
  "neutral",
  "info",
  "progress",
  "attention",
  "warning",
  "success",
  "danger",
] as const;

interface ToneClasses {
  /** Chip / badge surface: background + text + border */
  chip: string;
  /** Small status dot */
  dot: string;
  /** Text-only accent (values, icons) */
  text: string;
  /** Tinted panel (callouts, KPI tile accent background) */
  panel: string;
  /** Solid bar segment (stacked bars, progress) */
  bar: string;
}

export const mfTone: Record<Tone, ToneClasses> = {
  neutral: {
    chip: "border-tone-neutral-border bg-tone-neutral-bg text-tone-neutral-fg",
    dot: "bg-tone-neutral-fg",
    text: "text-tone-neutral-fg",
    panel: "border-tone-neutral-border bg-tone-neutral-bg",
    bar: "bg-tone-neutral-fg",
  },
  info: {
    chip: "border-tone-info-border bg-tone-info-bg text-tone-info-fg",
    dot: "bg-tone-info-fg",
    text: "text-tone-info-fg",
    panel: "border-tone-info-border bg-tone-info-bg",
    bar: "bg-tone-info-fg",
  },
  progress: {
    chip: "border-tone-progress-border bg-tone-progress-bg text-tone-progress-fg",
    dot: "bg-tone-progress-fg",
    text: "text-tone-progress-fg",
    panel: "border-tone-progress-border bg-tone-progress-bg",
    bar: "bg-tone-progress-fg",
  },
  attention: {
    chip: "border-tone-attention-border bg-tone-attention-bg text-tone-attention-fg",
    dot: "bg-tone-attention-fg",
    text: "text-tone-attention-fg",
    panel: "border-tone-attention-border bg-tone-attention-bg",
    bar: "bg-tone-attention-fg",
  },
  warning: {
    chip: "border-tone-warning-border bg-tone-warning-bg text-tone-warning-fg",
    dot: "bg-tone-warning-fg",
    text: "text-tone-warning-fg",
    panel: "border-tone-warning-border bg-tone-warning-bg",
    bar: "bg-tone-warning-fg",
  },
  success: {
    chip: "border-tone-success-border bg-tone-success-bg text-tone-success-fg",
    dot: "bg-tone-success-fg",
    text: "text-tone-success-fg",
    panel: "border-tone-success-border bg-tone-success-bg",
    bar: "bg-tone-success-fg",
  },
  danger: {
    chip: "border-tone-danger-border bg-tone-danger-bg text-tone-danger-fg",
    dot: "bg-tone-danger-fg",
    text: "text-tone-danger-fg",
    panel: "border-tone-danger-border bg-tone-danger-bg",
    bar: "bg-tone-danger-fg",
  },
};

/** Work-order lifecycle status → tone (Option B codes and legacy aliases). */
export const WO_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  planning: "info",
  ready: "info",
  in_progress: "progress",
  on_hold: "attention",
  completed: "success",
  closed: "neutral",
  cancelled: "danger",
  awaiting_approval: "attention",
  planned: "info",
  released: "info",
  ready_to_schedule: "info",
  assigned: "info",
  waiting_for_prerequisite: "attention",
  paused: "attention",
  mechanically_complete: "success",
  technically_verified: "success",
};

/** Intervention-request (DI) status → tone. */
export const DI_STATUS_TONE: Record<string, Tone> = {
  submitted: "info",
  in_review: "progress",
  returned_for_clarification: "warning",
  awaiting_approval: "attention",
  approved: "success",
  deferred: "neutral",
  closed: "neutral",
};

/** DI reported / validated urgency → tone. */
export const DI_URGENCY_TONE: Record<string, Tone> = {
  low: "neutral",
  medium: "warning",
  high: "attention",
  critical: "danger",
};

/** Urgency / priority level 1 (lowest) … 5 (emergency) → tone. */
export const URGENCY_TONE: Record<string, Tone> = {
  "1": "neutral",
  "2": "info",
  "3": "warning",
  "4": "attention",
  "5": "danger",
};

export function toneOf(map: Record<string, Tone>, code: string | null | undefined): Tone {
  if (!code) return "neutral";
  return map[code] ?? "neutral";
}

/** Chip classes for a status code through a tone map (drop-in for legacy `*_STYLE` maps). */
export function toneChipMap(map: Record<string, Tone>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(map).map(([code, tone]) => [code, `border ${mfTone[tone].chip}`]),
  );
}

/**
 * Pure rules of the inspection runner (no React). The numeric classification mirrors
 * `eval_numeric_status` in `src-tauri/src/inspection/results.rs`, which stays the authority:
 * the UI only previews what the backend will record.
 */
import type { InspectionCheckpoint, InspectionRunnerCheckpoint } from "@shared/ipc-types";

export type ResultStatus = "pass" | "warning" | "fail" | "not_accessible" | "not_done";

/** Round statuses in which results may still be recorded (backend `ROUND_OPEN_STATUSES`). */
export const ROUND_OPEN_STATUSES: ReadonlySet<string> = new Set([
  "scheduled",
  "released",
  "in_progress",
]);

/** Results that say the check was not performed: a reason is required. */
export const NOT_PERFORMED: ReadonlySet<string> = new Set(["not_accessible", "not_done"]);

export const ROUND_FILTERS: Record<"todo" | "toReview" | "done" | "all", string[]> = {
  todo: ["scheduled", "released", "in_progress"],
  toReview: ["completed_with_findings", "completed"],
  done: ["reviewed", "completed"],
  all: [],
};

export type RoundFilterKey = keyof typeof ROUND_FILTERS;

/** Maximum photo size accepted by the backend (`MAX_PHOTO_BYTES`). */
export const MAX_PHOTO_MB = 15;

export const CHECK_TYPES = ["pass_fail", "boolean", "numeric", "observation"] as const;
export type CheckType = (typeof CHECK_TYPES)[number];

function within(v: number, lo: number | null, hi: number | null): boolean {
  return (lo == null || v >= lo) && (hi == null || v <= hi);
}

type Limits = Pick<
  InspectionCheckpoint,
  "normal_min" | "normal_max" | "warning_min" | "warning_max"
>;

export function predictNumericStatus(value: number, cp: Limits): "pass" | "warning" | "fail" {
  const hasNormal = cp.normal_min != null || cp.normal_max != null;
  const hasWarning = cp.warning_min != null || cp.warning_max != null;
  if (!hasNormal && !hasWarning) return "pass";
  if (!hasNormal) return within(value, cp.warning_min, cp.warning_max) ? "pass" : "fail";
  if (within(value, cp.normal_min, cp.normal_max)) return "pass";
  if (!hasWarning) return "warning";
  return within(value, cp.warning_min, cp.warning_max) ? "warning" : "fail";
}

/** Parses a reading typed with a comma or a dot as decimal separator. */
export function parseReading(raw: string): number | null {
  const normalized = raw.trim().replace(/\s/g, "").replace(",", ".");
  if (normalized === "" || !/^[-+]?(\d+\.?\d*|\.\d+)$/.test(normalized)) return null;
  const n = Number(normalized);
  return Number.isFinite(n) ? n : null;
}

export type BandKind = "between" | "below" | "above";

/** Describes one limit band for display; `null` when the band is not defined. */
export function describeBand(
  min: number | null,
  max: number | null,
): { kind: BandKind; min: number | null; max: number | null } | null {
  if (min == null && max == null) return null;
  if (min != null && max != null) return { kind: "between", min, max };
  if (max != null) return { kind: "below", min: null, max };
  return { kind: "above", min, max: null };
}

/** The answer buttons offered for a check type, in display order. */
export function answerChoices(checkType: string): ResultStatus[] {
  switch (checkType) {
    case "pass_fail":
    case "boolean":
      return ["pass", "fail", "not_accessible", "not_done"];
    case "observation":
      return ["pass", "warning", "fail", "not_accessible", "not_done"];
    default:
      return ["not_accessible", "not_done"];
  }
}

/** Whether the round can be completed: every check answered with its evidence rules met. */
export function canComplete(checkpoints: InspectionRunnerCheckpoint[]): boolean {
  return checkpoints.length > 0 && checkpoints.every((c) => c.missing.length === 0);
}

export function pendingSyncCount(checkpoints: InspectionRunnerCheckpoint[]): {
  waiting: number;
  errors: number;
} {
  let waiting = 0;
  let errors = 0;
  for (const c of checkpoints) {
    if (c.sync_state === "waiting") waiting += 1;
    if (c.sync_state === "error" || c.sync_state === "conflict") errors += 1;
  }
  return { waiting, errors };
}

/** First checkpoint still needing something, or the first one. */
export function firstOpenIndex(checkpoints: InspectionRunnerCheckpoint[]): number {
  const i = checkpoints.findIndex((c) => c.missing.length > 0);
  return i < 0 ? 0 : i;
}

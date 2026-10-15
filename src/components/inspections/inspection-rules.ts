/**
 * Pure rules of the inspection module (no React). The numeric classification mirrors
 * `eval_numeric_status` in `src-tauri/src/inspection/results.rs`, which stays the authority:
 * the UI only previews what the backend will record.
 */
import type { Tone } from "@/design-system/tone";
import type {
  InspectionAnomalyRow,
  InspectionCheckpoint,
  InspectionRoundSummaryFilter,
  InspectionRunnerCheckpoint,
} from "@shared/ipc-types";

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

// ── Phase 6.3: tones, list filters, KPI drill-down, follow-up drafts, template checks ──

/** Round status → tone (one colour vocabulary, `design-system/tone.ts`). */
export const ROUND_STATUS_TONE: Record<string, Tone> = {
  scheduled: "info",
  released: "info",
  in_progress: "progress",
  completed: "success",
  completed_with_findings: "attention",
  reviewed: "success",
  missed: "danger",
  cancelled: "neutral",
};

/** Result of a checkpoint → tone. */
export const RESULT_TONE: Record<string, Tone> = {
  pass: "success",
  warning: "warning",
  fail: "danger",
  not_accessible: "neutral",
  not_done: "neutral",
};

/** Anomaly resolution status → tone. */
export const ANOMALY_STATUS_TONE: Record<string, Tone> = {
  open: "attention",
  triaged: "info",
  deferred: "neutral",
  resolved: "success",
  cancelled: "neutral",
};

/** Every round status, in lifecycle order (backend `ROUND_STATUSES`). */
export const ROUND_STATUSES = [
  "scheduled",
  "released",
  "in_progress",
  "completed",
  "completed_with_findings",
  "reviewed",
  "missed",
  "cancelled",
] as const;

/** Round statuses counted as done by the KPIs (backend `insights::DONE_STATUSES`). */
export const DONE_STATUSES = ["completed", "completed_with_findings", "reviewed"] as const;

export const ANOMALY_STATUSES = ["open", "triaged", "deferred", "resolved", "cancelled"] as const;

/** Status groups offered in the round filter, before the individual statuses. */
export const ROUND_STATUS_GROUPS: Record<string, readonly string[]> = {
  todo: ROUND_FILTERS.todo,
  toReview: ROUND_FILTERS.toReview,
  finished: DONE_STATUSES,
};

/** `overdue`: still to do and past its planned time; `period`: planned in the KPI period. */
export type RoundTiming = "overdue" | "period";

export interface RoundListQuery {
  /** A status group key, a single status, or null for every status. */
  status: string | null;
  timing: RoundTiming | null;
  /** The KPI period (RFC 3339), used when `timing` is `period`. */
  period: { from: string; to: string } | null;
  search: string;
}

/** The backend filter of a round list query (same conditions as the KPI behind it). */
export function roundFilterFor(q: RoundListQuery): InspectionRoundSummaryFilter {
  let statuses: string[] | null = null;
  if (q.status != null) {
    statuses = [...(ROUND_STATUS_GROUPS[q.status] ?? [q.status])];
  } else if (q.timing === "period") {
    // A planned period never counts cancelled rounds (`insights::get_inspection_kpis`).
    statuses = ROUND_STATUSES.filter((s) => s !== "cancelled");
  }
  const filter: InspectionRoundSummaryFilter = { statuses };
  if (q.timing === "overdue") filter.overdue_only = true;
  if (q.timing === "period" && q.period) {
    filter.due_from = q.period.from;
    filter.due_to = q.period.to;
  }
  const search = q.search.trim();
  if (search) filter.search = search;
  return filter;
}

export type InspectionKpiKey = "due" | "done" | "overdue" | "missed" | "compliance" | "anomalies";

export interface KpiDrillDown {
  tab: "rounds" | "anomalies";
  status: string | null;
  timing: RoundTiming | null;
  anomalyStatus: string | null;
}

/** Where a KPI tile leads: the tab and the filter that reproduce its figure. */
export function kpiDrillDown(key: InspectionKpiKey): KpiDrillDown {
  switch (key) {
    case "due":
    case "compliance":
      return { tab: "rounds", status: null, timing: "period", anomalyStatus: null };
    case "done":
      return { tab: "rounds", status: "finished", timing: "period", anomalyStatus: null };
    case "missed":
      return { tab: "rounds", status: "missed", timing: "period", anomalyStatus: null };
    case "overdue":
      return { tab: "rounds", status: null, timing: "overdue", anomalyStatus: null };
    case "anomalies":
      return { tab: "anomalies", status: null, timing: null, anomalyStatus: "open" };
  }
}

/** Mirrors `results::violated_limit`: the limit a reading crossed (warning limit first). */
export function violatedLimit(
  value: number,
  cp: Limits,
): { direction: "above" | "below"; limit: number } | null {
  const beyond = (lo: number | null, hi: number | null) => {
    if (hi != null && value > hi) return { direction: "above" as const, limit: hi };
    if (lo != null && value < lo) return { direction: "below" as const, limit: lo };
    return null;
  };
  return beyond(cp.warning_min, cp.warning_max) ?? beyond(cp.normal_min, cp.normal_max);
}

/** A reading outside its limits always needs a comment (backend `reading_needs_comment`). */
export function readingNeedsComment(checkType: string, status: string | null): boolean {
  return checkType === "numeric" && (status === "warning" || status === "fail");
}

/** i18next `t` as the pure helpers use it (options always given). */
export type Translate = (key: string, opts: Record<string, unknown>) => string;

/** Draft title and description of the request / work order written from an anomaly. */
export function followUpDraft(
  row: InspectionAnomalyRow,
  t: Translate,
  formatNumber: (v: number) => string,
  formatDateTime: (v: string | null) => string,
): { title: string; description: string } {
  const label = row.checkpoint_description?.trim() || row.checkpoint_code || row.template_name;
  const asset = row.asset_code ?? null;
  const suffix = asset ? "" : "NoAsset";
  const unit = row.measurement_unit ? ` ${row.measurement_unit}` : "";
  let title: string;
  if (row.numeric_value != null && row.violation_direction && row.violation_limit != null) {
    title = t(`followUp.title.${row.violation_direction}${suffix}`, {
      label,
      asset,
      value: `${formatNumber(row.numeric_value)}${unit}`,
      limit: `${formatNumber(row.violation_limit)}${unit}`,
    });
  } else if (row.anomaly.anomaly_type === "missing_mandatory_photo") {
    title = t(`followUp.title.photo${suffix}`, { label, asset });
  } else {
    const result = row.result_status ? t(`result.${row.result_status}`, {}) : "";
    title = t(`followUp.title.finding${suffix}`, { label, asset, result });
  }
  const lines = [
    t("followUp.description.round", {
      name: row.template_name,
      code: row.template_code,
      date: formatDateTime(row.round_scheduled_at),
    }),
    t("followUp.description.checkpoint", { code: row.checkpoint_code ?? "", label }),
  ];
  if (asset) lines.push(t("followUp.description.asset", { asset, name: row.asset_name ?? "" }));
  if (row.numeric_value != null) {
    lines.push(
      t("followUp.description.reading", {
        value: `${formatNumber(row.numeric_value)}${unit}`,
        status: row.result_status ? t(`result.${row.result_status}`, {}) : "",
      }),
    );
  } else if (row.result_status) {
    lines.push(t("followUp.description.result", { status: t(`result.${row.result_status}`, {}) }));
  }
  const band = (min: number | null, max: number | null): string | null => {
    const b = describeBand(min, max);
    if (!b) return null;
    if (b.kind === "between" && b.min != null && b.max != null)
      return `${formatNumber(b.min)} – ${formatNumber(b.max)}${unit}`;
    if (b.kind === "below" && b.max != null) return `≤ ${formatNumber(b.max)}${unit}`;
    if (b.min != null) return `≥ ${formatNumber(b.min)}${unit}`;
    return null;
  };
  const normal = band(row.normal_min, row.normal_max);
  const warning = band(row.warning_min, row.warning_max);
  if (normal) lines.push(t("followUp.description.normal", { band: normal }));
  if (warning) lines.push(t("followUp.description.warning", { band: warning }));
  const observation = row.text_value?.trim();
  if (observation) lines.push(t("followUp.description.observation", { text: observation }));
  const comment = row.result_comment?.trim();
  if (comment) lines.push(t("followUp.description.comment", { comment }));
  return { title, description: lines.join("\n") };
}

/** One problem of a checklist row in the template editor (translated by its code). */
export interface CheckRowIssue {
  code: "codeRequired" | "codeDuplicate" | "limitInvalid" | "limitsOrder" | "warningBand";
  field?: LimitField;
}

export const LIMIT_FIELDS = ["normalMin", "normalMax", "warningMin", "warningMax"] as const;
export type LimitField = (typeof LIMIT_FIELDS)[number];

export interface CheckRowInput {
  code: string;
  checkType: string;
  normalMin: string;
  normalMax: string;
  warningMin: string;
  warningMax: string;
}

/**
 * Same rules as `queries::validate_drafts` (the backend stays the authority), checked before
 * saving so the editor can say which row is wrong and why. Returns the issues per row index.
 */
export function validateCheckRows(rows: CheckRowInput[]): Map<number, CheckRowIssue[]> {
  const out = new Map<number, CheckRowIssue[]>();
  const add = (i: number, issue: CheckRowIssue) => out.set(i, [...(out.get(i) ?? []), issue]);
  const seen = new Set<string>();
  rows.forEach((r, i) => {
    const code = r.code.trim();
    if (!code) {
      add(i, { code: "codeRequired" });
    } else if (seen.has(code.toUpperCase())) {
      add(i, { code: "codeDuplicate" });
    } else {
      seen.add(code.toUpperCase());
    }
    if (r.checkType !== "numeric") return;
    const v: Record<LimitField, number | null> = {
      normalMin: null,
      normalMax: null,
      warningMin: null,
      warningMax: null,
    };
    let invalid = false;
    for (const f of LIMIT_FIELDS) {
      if (r[f].trim() === "") continue;
      const n = parseReading(r[f]);
      if (n == null) {
        add(i, { code: "limitInvalid", field: f });
        invalid = true;
      } else {
        v[f] = n;
      }
    }
    if (invalid) return;
    const ordered = (lo: number | null, hi: number | null) => lo == null || hi == null || lo <= hi;
    if (!ordered(v.normalMin, v.normalMax) || !ordered(v.warningMin, v.warningMax)) {
      add(i, { code: "limitsOrder" });
      return;
    }
    const wideLow = v.warningMin == null || (v.normalMin != null && v.warningMin <= v.normalMin);
    const wideHigh = v.warningMax == null || (v.normalMax != null && v.warningMax >= v.normalMax);
    if (!wideLow || !wideHigh) add(i, { code: "warningBand" });
  });
  return out;
}

/** SVG polyline points of a value series drawn left (oldest) to right in a `w × h` box. */
export function sparklinePoints(values: number[], w: number, h: number): string {
  if (values.length === 0) return "";
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min;
  const step = values.length > 1 ? w / (values.length - 1) : 0;
  const round = (n: number) => Math.round(n * 10) / 10;
  return values
    .map((v, i) => {
      const x = values.length > 1 ? i * step : w / 2;
      // A flat series is drawn across the middle of the box.
      const y = span === 0 ? h / 2 : h - ((v - min) / span) * h;
      return `${round(x)},${round(y)}`;
    })
    .join(" ");
}

// ── Schedule form (mirrors `scheduling::validate_schedule_input`) ──

export const INTERVAL_UNITS = ["hour", "day", "week", "month", "year"] as const;
/** Backend limits (`scheduling::MAX_HORIZON_DAYS`, `scheduling::MAX_GRACE_HOURS`). */
export const MAX_HORIZON_DAYS = 366;
export const MAX_GRACE_HOURS = 744;

export interface ScheduleForm {
  code: string;
  name: string;
  templateId: string;
  triggerKind: "calendar" | "meter";
  intervalValue: string;
  intervalUnit: string;
  anchorLocal: string;
  meterId: number | null;
  meterInterval: string;
  meterStart: string;
  assigneeId: number | null;
  teamId: number | null;
  graceHours: string;
  horizonDays: string;
}

export type ScheduleField =
  | "code"
  | "name"
  | "templateId"
  | "intervalValue"
  | "anchorLocal"
  | "meterId"
  | "meterInterval"
  | "meterStart"
  | "assignee"
  | "graceHours"
  | "horizonDays";

function wholeNumber(raw: string): number | null {
  const n = parseReading(raw);
  return n != null && Number.isInteger(n) ? n : null;
}

/** Field problems of a schedule form, as translation keys under `schedules.errors`. */
export function scheduleFormIssues(f: ScheduleForm): Partial<Record<ScheduleField, string>> {
  const out: Partial<Record<ScheduleField, string>> = {};
  if (!f.code.trim()) out.code = "required";
  if (!f.name.trim()) out.name = "required";
  if (!f.templateId) out.templateId = "required";
  if (f.assigneeId == null && f.teamId == null) out.assignee = "assigneeRequired";
  const grace = wholeNumber(f.graceHours);
  if (grace == null || grace < 1 || grace > MAX_GRACE_HOURS) out.graceHours = "graceRange";
  if (f.triggerKind === "calendar") {
    const every = wholeNumber(f.intervalValue);
    if (every == null || every < 1) out.intervalValue = "intervalRequired";
    if (!f.anchorLocal || Number.isNaN(new Date(f.anchorLocal).getTime()))
      out.anchorLocal = "anchorRequired";
    const horizon = wholeNumber(f.horizonDays);
    if (horizon == null || horizon < 1 || horizon > MAX_HORIZON_DAYS)
      out.horizonDays = "horizonRange";
  } else {
    if (f.meterId == null) out.meterId = "meterRequired";
    const every = parseReading(f.meterInterval);
    if (every == null || every <= 0) out.meterInterval = "meterIntervalRequired";
    if (f.meterStart.trim() !== "" && parseReading(f.meterStart) == null)
      out.meterStart = "notNumber";
  }
  return out;
}

/** `YYYY-MM-DDTHH:MM` in local time, for the date-time field, from an RFC 3339 instant. */
export function isoToLocalInput(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

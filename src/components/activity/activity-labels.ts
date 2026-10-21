/**
 * Readable labels for activity and audit events (FR/EN): known codes are translated in the
 * `activity` namespace; an unknown code is shown "humanized" (dots and underscores as spaces),
 * never as a column name or raw JSON.
 */
import type { Tone } from "@/design-system/tone";

/** Any namespace-bound `t` (the keys used here live in the `activity` namespace). */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type TFunction = (key: any, options?: any) => unknown;

/** Source modules written by the backend, with aliases folded onto one label. */
const MODULE_ALIASES: Record<string, string> = {
  wo: "wo",
  work_orders: "wo",
  di: "di",
  intervention_requests: "di",
  pm: "pm",
  planning: "planning",
  permit: "permit",
  personnel: "personnel",
  inventory: "inventory",
  inspection: "inspection",
  notifications: "notifications",
  sync: "sync",
  rbac: "rbac",
  iot: "iot",
  archive: "archive",
  arc: "archive",
  auth: "auth",
  equipment: "equipment",
  org: "org",
  finance: "finance",
  reliability: "reliability",
};

export const ACTIVITY_MODULE_OPTIONS = [
  "wo",
  "di",
  "pm",
  "planning",
  "permit",
  "personnel",
  "inventory",
  "inspection",
  "notifications",
  "sync",
  "rbac",
  "iot",
  "archive",
] as const;

export const ACTIVITY_SEVERITY_OPTIONS = ["info", "warning", "error", "critical"] as const;
export const ACTIVITY_CLASS_OPTIONS = ["operational", "system", "security"] as const;
export const AUDIT_RESULT_OPTIONS = ["success", "applied", "blocked"] as const;
export const AUDIT_RETENTION_OPTIONS = ["standard", "operations", "compliance"] as const;

const SEVERITY_TONE: Record<string, Tone> = {
  info: "info",
  pass: "success",
  warning: "warning",
  warn: "warning",
  medium: "warning",
  error: "danger",
  fail: "danger",
  high: "danger",
  critical: "danger",
  blocker: "danger",
  blocking: "danger",
};

const RESULT_TONE: Record<string, Tone> = {
  success: "success",
  applied: "success",
  pass: "success",
  completed_no_findings: "success",
  completed_with_findings: "warning",
  blocked: "danger",
  failure: "danger",
  failed: "danger",
  denied: "danger",
  missed: "warning",
};

/** "wo.status_changed" → "wo status changed" (only when no translation exists). */
export function humanizeCode(code: string): string {
  return code.replace(/[._]+/g, " ").trim();
}

function translated(t: TFunction, key: string, code: string): string {
  return String(t(key, { defaultValue: humanizeCode(code) }));
}

export function moduleLabel(t: TFunction, code: string | null | undefined): string {
  if (!code) return String(t("labels.module.none"));
  const canonical = MODULE_ALIASES[code] ?? code;
  return translated(t, `labels.module.${canonical}`, code);
}

/**
 * Name of an event or audit action ("login.success" → "Signed in"). Known codes are translated;
 * a code this version does not know keeps the readable fallback ("sync repair execute"), never a
 * blank. Keys replace the code's dots with "__" (dots separate i18n key levels).
 */
export function eventLabel(t: TFunction, code: string | null | undefined): string {
  if (!code) return String(t("labels.module.none"));
  return translated(t, `labels.event.${code.replace(/\./g, "__")}`, code);
}

export function severityLabel(t: TFunction, code: string): string {
  return translated(t, `labels.severity.${code}`, code);
}

export function severityTone(code: string): Tone {
  return SEVERITY_TONE[code] ?? "neutral";
}

export function eventClassLabel(t: TFunction, code: string): string {
  return translated(t, `labels.eventClass.${code}`, code);
}

export function resultLabel(t: TFunction, code: string): string {
  return translated(t, `labels.result.${code}`, code);
}

export function resultTone(code: string): Tone {
  return RESULT_TONE[code] ?? "neutral";
}

export function authContextLabel(t: TFunction, code: string): string {
  return translated(t, `labels.authContext.${code}`, code);
}

export function retentionLabel(t: TFunction, code: string): string {
  return translated(t, `labels.retention.${code}`, code);
}

/** A details/summary payload as readable "label: value" lines (nested values summarised). */
export function payloadLines(payload: unknown): { key: string; value: string }[] {
  if (payload == null) return [];
  if (typeof payload !== "object" || Array.isArray(payload)) {
    return [{ key: "", value: formatValue(payload) }];
  }
  return Object.entries(payload as Record<string, unknown>)
    .filter(([, v]) => v !== null && v !== undefined && v !== "")
    .map(([k, v]) => ({ key: humanizeCode(k), value: formatValue(v) }));
}

function formatValue(v: unknown): string {
  if (v == null) return "—";
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  if (Array.isArray(v)) return v.map(formatValue).join(", ");
  return Object.entries(v as Record<string, unknown>)
    .map(([k, inner]) => `${humanizeCode(k)}: ${formatValue(inner)}`)
    .join(" · ");
}

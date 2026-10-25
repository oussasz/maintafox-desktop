/**
 * Text of a notification in the reader's language.
 *
 * The backend stores a message key and display parameters (`notification_events.message_key`,
 * `message_params_json`); this renders `notifications:messages.<key>.title|body` with the params
 * formatted for the reader (dates, numbers). Notifications written before message keys existed,
 * or whose key this version does not know, show the text stored at creation.
 */

import type { TFunction } from "i18next";

import type { Formatters } from "@/hooks/use-formatters";
import type { NotificationSummary } from "@/services/notification-service";
import { DATE_PARAMS, DATE_TIME_PARAMS } from "@/utils/date-params";

/** Message keys this version renders (`notifications:messages.*`). */
export const MESSAGE_KEYS = [
  "wo_assigned",
  "wo_overdue",
  "inspection_round_missed",
  "stock_critical",
  // Stock left below zero or counted twice by sync (inv.manage holders).
  "stock_negative",
  "stock_double_count",
  "ptw_expiring",
  "ptw_expired",
  "sync_conflict",
  "di_pending_review",
  "di_returned",
  "di_approved",
  "di_closed",
  "di_converted",
  "di_deferred",
  "di_sla_breach_response",
  "di_sla_breach_resolution",
  "pm_due",
  "pm_missed",
  "pm_deferred",
  "pm_follow_up_created",
  "cert_expiry",
  "cert_expired",
  "personnel_critical_block",
  "daily_digest",
  // A safety rule changed on another PC, applied here (settings administrators).
  "site_rule_received_on",
  "site_rule_received_off",
  "three_person_received_on",
  "three_person_received_off",
  // A lock taken off by a forced removal (permit close-right holders of the site).
  "loto_forced_removal",
  // Two concurrent versions of a PM plan, or a due date planned twice by them (PM planners).
  "pm_concurrent_versions",
  "pm_duplicate_occurrence",
  // Grants received by sync from an author who lost the right (administrators).
  "admin_grants_to_review",
  "grants_from_former_authors",
  // A password change or account re-enable received without a right author (administrators).
  "password_change_refused",
  "password_change_from_older_version",
  "account_reenable_refused",
  // A running work order put on hold by its permit's suspension (its responsible).
  "permit_suspended_work_held",
  // A permit ended on another PC before or while its work ran here (responsible, close-right holders).
  "permit_ended_before_start",
  "permit_ended_during_work",
  // The same when the end has no time (a cancellation), and a suspension lifted around a start.
  "permit_ended_before_start_untimed",
  "permit_ended_during_work_untimed",
  "permit_suspended_around_start",
  "permit_suspended_during_work",
] as const;

type Fmt = Pick<Formatters, "formatDate" | "formatDateTime" | "formatNumber">;

/** `t` for keys built at run time (message keys, audience codes) with interpolation values. */
export type LooseT = (key: string, options?: Record<string, unknown>) => string;
export function looseT(t: TFunction<"notifications">): LooseT {
  return t as unknown as LooseT;
}

/** Display value of one parameter ("—" when missing). */
export function formatParam(name: string, value: unknown, fmt: Fmt): string {
  if (value === null || value === undefined || value === "") return "—";
  if (typeof value === "number") return fmt.formatNumber(value);
  if (typeof value === "string") {
    if (DATE_TIME_PARAMS.has(name)) return fmt.formatDateTime(value);
    if (DATE_PARAMS.has(name)) return fmt.formatDate(value);
    return value;
  }
  return String(value);
}

function isKnownKey(key: string | null): key is (typeof MESSAGE_KEYS)[number] {
  return key != null && (MESSAGE_KEYS as readonly string[]).includes(key);
}

export interface NotificationText {
  title: string;
  body: string | null;
}

/** Title and body of a notification for the reader. */
export function notificationText(
  row: Pick<NotificationSummary, "title" | "body" | "message_key" | "message_params">,
  t: TFunction<"notifications">,
  fmt: Fmt,
): NotificationText {
  if (!isKnownKey(row.message_key)) return { title: row.title, body: row.body };
  const tr = looseT(t);
  const raw = row.message_params ?? {};
  const params: Record<string, string> = {};
  for (const [name, value] of Object.entries(raw)) {
    if (name === "counts") continue;
    params[name] = formatParam(name, value, fmt);
  }
  if (row.message_key === "daily_digest") {
    const counts = (raw["counts"] ?? {}) as Record<string, unknown>;
    const parts = Object.entries(counts)
      .filter(([, n]) => typeof n === "number" && n > 0)
      .map(([code, n]) => `${categoryLabel(code, t)} : ${fmt.formatNumber(n as number)}`);
    return {
      title: tr("messages.daily_digest.title", params),
      body: parts.length > 0 ? parts.join(" · ") : null,
    };
  }
  return {
    title: tr(`messages.${row.message_key}.title`, params),
    body: tr(`messages.${row.message_key}.body`, params),
  };
}

export function categoryLabel(code: string, t: TFunction<"notifications">): string {
  return t(`category.${code}`, { defaultValue: t("category.other") });
}

/** Why the reader received it, or null when not recorded (older notifications). */
export function notificationReason(
  row: Pick<NotificationSummary, "reason_code" | "reason_ref">,
  t: TFunction<"notifications">,
): string | null {
  const code = row.reason_code;
  if (!code) return null;
  if (code === "permission") {
    return t("reason.permission", { permission: row.reason_ref ?? "" });
  }
  if (code === "escalation") {
    const [level, minutes] = (row.reason_ref ?? "").split(":");
    return t("reason.escalation", { level: level ?? "", minutes: minutes ?? "" });
  }
  return t(`reason.${code}`, { defaultValue: t("reason.other") });
}

/** "Tomorrow 08:00" in the reader's time zone, for the snooze menu. */
export function tomorrowMorning(now: Date, hour: number): Date {
  const d = new Date(now);
  d.setDate(d.getDate() + 1);
  d.setHours(hour, 0, 0, 0);
  return d;
}

/** Start and end of a local calendar day ("YYYY-MM-DD") as RFC 3339 instants. */
export function localDayBounds(day: string, end: boolean): string | undefined {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return undefined;
  const d = end
    ? new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), 23, 59, 59, 999)
    : new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), 0, 0, 0, 0);
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString();
}

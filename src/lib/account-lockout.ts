/**
 * The account lockout (`AppError::AccountLocked`, code `ACCOUNT_LOCKED`) in the user's words.
 *
 * The server sends the end of the lockout as data (`details[0].params.until`, a UTC RFC 3339
 * instant) beside its technical English sentence. Every screen that shows the lockout (sign-in,
 * the sign-in-again dialog, lock screen, step-up, PIN and password change) builds its text here,
 * at display time, so it is in the app language, in local time, and follows a language switch.
 */
import { i18n } from "@/i18n/config";
import { storedInstant } from "@/utils/date-params";
import { formatDateTime, formatTime } from "@/utils/formatters";

/** Server code (`AppError::AccountLocked`): the account is locked out after wrong passwords. */
export const ACCOUNT_LOCKED = "ACCOUNT_LOCKED";

/** The end of the lockout the server sent with an `ACCOUNT_LOCKED` refusal; null when absent. */
export function lockoutUntil(err: unknown): string | null {
  if (typeof err !== "object" || err === null || !("details" in err)) return null;
  const details = (err as { details: unknown }).details;
  if (!Array.isArray(details)) return null;
  for (const item of details) {
    if (typeof item !== "object" || item === null) continue;
    const row = item as { code?: unknown; params?: unknown };
    if (row.code !== ACCOUNT_LOCKED) continue;
    const params = row.params;
    if (typeof params !== "object" || params === null) continue;
    const until = (params as { until?: unknown }).until;
    if (typeof until === "string" && until.trim() !== "") return until;
  }
  return null;
}

function sameLocalDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/**
 * "Account locked after too many wrong passwords, until 10:15." in the app language: the time
 * alone when the lockout ends today, the date and time when it ends another day, no time when
 * the end is missing or unreadable.
 */
export function accountLockedText(
  until: string | null | undefined,
  now: Date = new Date(),
): string {
  const instant = until ? storedInstant(until) : null;
  if (!instant) return String(i18n.t("auth:lockout.lockedNoTime"));
  // "fr" | "en" (the formatters' locale codes), also when i18next reports a region ("en-US").
  const language = (i18n.resolvedLanguage || i18n.language || "").split("-")[0] ?? "";
  return sameLocalDay(instant, now)
    ? String(i18n.t("auth:lockout.lockedUntilTime", { time: formatTime(instant, language) }))
    : String(
        i18n.t("auth:lockout.lockedUntilDateTime", {
          dateTime: formatDateTime(instant, language),
        }),
      );
}

/** The lockout text for an `ACCOUNT_LOCKED` refusal (the IPC reject object). */
export function accountLockedErrorText(err: unknown): string {
  return accountLockedText(lockoutUntil(err));
}

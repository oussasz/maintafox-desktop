/**
 * Extract a human-readable message from an unknown error value.
 *
 * Tauri's invoke() rejects with plain objects like `{ code, message }`,
 * not Error instances.  `String(obj)` yields "[object Object]" which is
 * useless in the UI.  This helper covers the common shapes:
 *
 *  - Error instances → err.message
 *  - Objects with a `message` string → obj.message
 *  - Strings → as-is
 *  - Everything else → JSON.stringify fallback
 */
import { formatOrgIssuesWithI18n, type OrgIssueLike } from "@/lib/format-org-validation-issue";

export function toErrorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  if (typeof err === "object" && err !== null && "message" in err) {
    const msg = (err as { message: unknown }).message;
    if (typeof msg === "string") return msg;
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

/** Structured validation issues from AppError IPC `details` when present. */
export function extractIpcValidationDetails(err: unknown): OrgIssueLike[] {
  if (typeof err !== "object" || err === null || !("details" in err)) {
    return [];
  }
  const details = (err as { details: unknown }).details;
  if (!Array.isArray(details)) return [];

  const issues: OrgIssueLike[] = [];
  for (const item of details) {
    if (typeof item !== "object" || item === null) continue;
    const row = item as Record<string, unknown>;
    const message = typeof row.message === "string" ? row.message : "";
    if (!message && typeof row.code !== "string") continue;
    const params =
      typeof row.params === "object" &&
      row.params !== null &&
      !Array.isArray(row.params)
        ? Object.fromEntries(
            Object.entries(row.params as Record<string, unknown>).filter(
              (entry): entry is [string, string] => typeof entry[1] === "string",
            ),
          )
        : {};
    issues.push({
      code: typeof row.code === "string" ? row.code : "",
      message: message || (typeof row.code === "string" ? row.code : "Validation failed"),
      params,
    });
  }
  return issues;
}

/**
 * Prefer localized org validation details; strip Debug-style "Validation failed: " when falling back.
 */
export function formatOrgIpcError(err: unknown): string {
  const details = extractIpcValidationDetails(err);
  const coded = details.filter((d) => d.code);
  if (coded.length > 0) {
    return formatOrgIssuesWithI18n(coded);
  }
  if (details.length > 0) {
    return details.map((d) => d.message).join("; ");
  }
  return toErrorMessage(err).replace(/^Validation failed:\s*/i, "");
}

/** Tauri IPC error code when present on a plain reject object. */
export function extractIpcErrorCode(err: unknown): string | null {
  if (typeof err === "object" && err !== null && "code" in err) {
    const code = (err as { code: unknown }).code;
    return typeof code === "string" && code.trim() ? code.trim() : null;
  }
  return null;
}

/**
 * True when an IPC/API failure means there is no authenticated session yet.
 * Must not be confused with "entitlement envelope is missing".
 */
export function isSessionUnavailableError(err: unknown): boolean {
  const code = (extractIpcErrorCode(err) ?? "").toUpperCase();
  // SESSION_LOCKED is idle-lock with a live session — not "unavailable".
  if (
    code === "AUTH_ERROR" ||
    code === "SESSION_CLAIM_INVALID" ||
    code === "ACCOUNT_LOCKED" ||
    code === "TENANT_SCOPE_VIOLATION"
  ) {
    return true;
  }
  const msg = toErrorMessage(err).toLowerCase();
  return (
    msg.includes("not authenticated") ||
    msg.includes("no active session") ||
    msg.includes("session required") ||
    msg.includes("authentication required") ||
    msg.includes("requires an authenticated session") ||
    msg.includes("session expirée") ||
    msg.includes("session expiree") ||
    msg.includes("session expired") ||
    msg.includes("veuillez vous reconnecter")
  );
}

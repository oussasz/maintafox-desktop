import type { AdminChangeEventDetail } from "@shared/ipc-types";

type Translate = (key: string, options?: Record<string, unknown>) => string;

/** Actions whose stored summary is the reason a person typed (shown as such, never as a template). */
const REASON_ACTIONS = new Set(["user_deactivated", "user_reactivated"]);

/** Refusal codes the backend stores in place of its own sentence (`record_blocked_admin_action`). */
const REFUSAL_CODES = new Set(["permission_denied", "validation_failed", "not_found"]);

function listLength(json: string | null, key: string): number {
  if (!json) return 0;
  try {
    const v = (JSON.parse(json) as Record<string, unknown>)[key];
    return Array.isArray(v) ? v.length : 0;
  } catch {
    return 0;
  }
}

/**
 * The sentence shown under an administration journal entry, written here in the user's language
 * from the action, its target and its recorded changes. The stored summary is not displayed: older
 * entries hold English sentences and refusals held the backend's own error text.
 * `undefined` when the action label already says everything.
 */
export function describeAdminEvent(
  evt: AdminChangeEventDetail,
  target: string | null,
  t: Translate,
): string | undefined {
  if (evt.apply_result === "blocked") {
    const code = REFUSAL_CODES.has(evt.summary ?? "") ? (evt.summary as string) : "generic";
    return t(`console.refusal.${code}`);
  }
  if (REASON_ACTIONS.has(evt.action)) {
    return evt.summary ? t("console.reasonLine", { reason: evt.summary }) : undefined;
  }
  const name = target ?? "";
  switch (evt.action) {
    case "role_updated":
      return t("console.summary.role_updated", {
        target: name,
        added: listLength(evt.diff_json, "permissions_added"),
        removed: listLength(evt.diff_json, "permissions_removed"),
      });
    case "user_created":
    case "user_updated":
    case "password_reset_by_admin":
    case "account_unlocked":
    case "role_created":
    case "role_retired":
    case "role_assigned":
    case "role_revoked":
    case "emergency_grant_created":
    case "permission_created":
      return t(`console.summary.${evt.action}`, { target: name });
    default:
      return undefined;
  }
}

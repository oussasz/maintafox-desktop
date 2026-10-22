/**
 * Turns any failure (Tauri IPC reject object, Error, string) into user-facing text.
 *
 * One mapping for the whole app: the global IPC toast (`ipc-invoke.ts`) and inline error
 * states (`ErrorState`) both use it, so the same failure reads the same everywhere.
 *  - title: translated message for the error code or gate code (`errors:appError.*`,
 *    `errors:gateError.*`); the backend text only when no translation exists.
 *  - description: localized validation details, else the backend text without the
 *    "Validation failed:" prefix, when it adds information to the title.
 *  - technical: the backend's own text, for a "Détails techniques" disclosure and the support
 *    log only. A GATE code never appears in the title or description, translated or not.
 *  - a response that fails schema validation (ZodError) gets a plain message; the technical
 *    detail goes to the console, never on screen.
 */
import { ZodError } from "zod";

import { i18n } from "@/i18n/config";
import { STEP_UP_CANCELLED_MESSAGE } from "@/lib/step-up-bridge";
import { formatOrgIssuesWithI18n } from "@/lib/format-org-validation-issue";
import {
  extractIpcErrorCode,
  extractIpcValidationDetails,
  formatOrgIpcError,
  toErrorMessage,
} from "@/utils/errors";

export interface AppErrorDescription {
  title: string;
  description: string | null;
  code: string | null;
  /** The backend's raw text, shown only inside a "Détails techniques" disclosure. */
  technical: string | null;
}

/** One line for an inline error: the coded reasons when there are some, else the title. */
export function appErrorText(err: unknown): string {
  const { title, description } = describeAppError(err);
  return description ?? title;
}

export function describeAppError(err: unknown): AppErrorDescription {
  if (err instanceof ZodError) {
    console.error("[describeAppError] response did not match its schema", err.issues);
    return {
      title: String(i18n.t("errors:generic.invalidData")),
      description: String(i18n.t("errors:generic.contactSupport")),
      code: "INVALID_RESPONSE",
      technical: null,
    };
  }
  const code = extractIpcErrorCode(err);
  const rawMsg = toErrorMessage(err);
  if (rawMsg === STEP_UP_CANCELLED_MESSAGE) {
    return {
      title: String(i18n.t("auth:stepUp.cancelled")),
      description: null,
      code: "STEP_UP_CANCELLED",
    };
  }

  const i18nKey = code ? `errors:appError.${code}` : "";
  const gateCodeMatch = rawMsg.match(/(GATE_[A-Z0-9_]+)(?::[A-Za-z0-9_=,\-./]+)?/);
  const gateCode = gateCodeMatch?.[1] ?? "";
  const gateI18nKey = gateCode ? `errors:gateError.${gateCode}` : "";
  const fallback = rawMsg.replace(/^Validation failed:\s*/i, "").trim();
  // An untranslated GATE code never reaches the user: the generic text of the error code
  // stands in and the raw text goes to the technical disclosure.
  const codeTitle = i18nKey && i18n.exists(i18nKey) ? String(i18n.t(i18nKey)) : "";
  const title =
    gateI18nKey && i18n.exists(gateI18nKey)
      ? String(i18n.t(gateI18nKey))
      : gateCode
        ? codeTitle || String(i18n.t("errors:generic.unexpected"))
        : codeTitle || fallback || String(i18n.t("errors:generic.unexpected"));

  const details = extractIpcValidationDetails(err);
  const coded = details.filter((d) => d.code);
  let description: string | null = null;
  if (coded.length > 0) {
    description = formatOrgIssuesWithI18n(coded);
  } else if (gateCode) {
    description = null;
  } else if (details.length > 0) {
    description = details.map((d) => d.message).join("; ");
  } else if (title !== fallback && fallback) {
    description = formatOrgIpcError(err);
  }
  if (description && description === title) description = null;

  const technical = fallback && fallback !== title && fallback !== description ? fallback : null;
  return { title, description, code, technical };
}

/**
 * One line saying why an action was refused: the translated rule (coded validation details)
 * when the backend gave one, otherwise the title. For inline messages and screen-reader
 * announcements, where a generic "Validation failed." alone would hide the reason.
 */
export function appErrorReason(err: unknown): string {
  const described = describeAppError(err);
  return described.code === "VALIDATION_FAILED" && described.description
    ? described.description
    : described.title;
}

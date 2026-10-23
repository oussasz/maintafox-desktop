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
import type { TOptions } from "i18next";
import { ZodError } from "zod";

import { i18n } from "@/i18n/config";
import { formatOrgIssuesWithI18n } from "@/lib/format-org-validation-issue";
import { STEP_UP_CANCELLED_MESSAGE } from "@/lib/step-up-bridge";
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
      technical: null,
    };
  }

  // A refused right: the translated refusal, never the server's own (English, technical) text.
  // The rights have no readable names in the locale data, so the generic reason is given.
  if (code === "PERMISSION_DENIED") {
    return {
      title: String(i18n.t("errors:appError.PERMISSION_DENIED")),
      description: String(i18n.t("errors:generic.forbidden")),
      code,
      // The server's text names the missing right: kept for "Détails techniques" and support.
      technical: rawMsg.trim() || null,
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
  const details = extractIpcValidationDetails(err);
  // The parameters the backend sent with the GATE code fill its text ("de {{from}} à {{to}}").
  const gateParams = details.find((d) => d.code === gateCode)?.params ?? {};
  const title =
    gateI18nKey && i18n.exists(gateI18nKey)
      ? String(i18n.t(gateI18nKey, gateParams as TOptions))
      : gateCode
        ? codeTitle || String(i18n.t("errors:generic.unexpected"))
        : codeTitle || fallback || String(i18n.t("errors:generic.unexpected"));

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
 * The translated text of the GATE code found in a backend sentence; the generic text when the
 * code has no translation (a GATE code never reaches the user); null when there is no GATE code.
 */
function gateText(message: string): string | null {
  const code = message.match(/(GATE_[A-Z0-9_]+)(?::[A-Za-z0-9_=,\-./]+)?/)?.[1];
  if (!code) return null;
  const key = `errors:gateError.${code}`;
  return i18n.exists(key) ? String(i18n.t(key)) : String(i18n.t("errors:generic.unexpected"));
}

/**
 * Every reason of a refusal as its own translated line (a close-out refusal lists several rules).
 * Each detail is translated by its code, or by the GATE code its sentence starts with; a detail
 * with neither keeps the backend sentence until its rule gets a code. A refusal with no details
 * is one line, as `appErrorReason`.
 */
export function appErrorLines(err: unknown): string[] {
  const details = extractIpcValidationDetails(err);
  if (details.length === 0) return [appErrorReason(err)];
  const lines = details.map((d) => {
    if (d.code) return formatOrgIssuesWithI18n([d]);
    return gateText(d.message) ?? d.message.replace(/^Validation failed:\s*/i, "");
  });
  return lines.filter((l) => l.trim() !== "");
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

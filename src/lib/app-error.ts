/**
 * Turns any failure (Tauri IPC reject object, Error, string) into user-facing text.
 *
 * One mapping for the whole app: the global IPC toast (`ipc-invoke.ts`) and inline error
 * states (`ErrorState`) both use it, so the same failure reads the same everywhere.
 *  - title: translated message for the error code or gate code (`errors:appError.*`,
 *    `errors:gateError.*`); the backend text only when no translation exists.
 *  - description: localized validation details, else the backend text without the
 *    "Validation failed:" prefix, when it adds information to the title.
 *  - a response that fails schema validation (ZodError) gets a plain message; the technical
 *    detail goes to the console, never on screen.
 */
import { ZodError } from "zod";

import { i18n } from "@/i18n/config";
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
}

export function describeAppError(err: unknown): AppErrorDescription {
  if (err instanceof ZodError) {
    console.error("[describeAppError] response did not match its schema", err.issues);
    return {
      title: String(i18n.t("errors:generic.invalidData")),
      description: String(i18n.t("errors:generic.contactSupport")),
      code: "INVALID_RESPONSE",
    };
  }
  const code = extractIpcErrorCode(err);
  const rawMsg = toErrorMessage(err);

  const i18nKey = code ? `errors:appError.${code}` : "";
  const gateCodeMatch = rawMsg.match(/(GATE_[A-Z0-9_]+)(?::[A-Za-z0-9_=,\-./]+)?/);
  const gateCode = gateCodeMatch?.[1] ?? "";
  const gateI18nKey = gateCode ? `errors:gateError.${gateCode}` : "";
  const fallback = rawMsg.replace(/^Validation failed:\s*/i, "").trim();
  const title =
    gateI18nKey && i18n.exists(gateI18nKey)
      ? String(i18n.t(gateI18nKey))
      : i18nKey && i18n.exists(i18nKey)
        ? String(i18n.t(i18nKey))
        : fallback || String(i18n.t("errors:generic.unexpected"));

  const details = extractIpcValidationDetails(err);
  const coded = details.filter((d) => d.code);
  let description: string | null = null;
  if (coded.length > 0) {
    description = formatOrgIssuesWithI18n(coded);
  } else if (details.length > 0) {
    description = details.map((d) => d.message).join("; ");
  } else if (title !== fallback && fallback) {
    description = formatOrgIpcError(err);
  }
  if (description && description === title) description = null;

  return { title, description, code };
}

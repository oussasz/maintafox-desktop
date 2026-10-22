/**
 * Localize org validation / mutation issues for end users.
 * Prefer i18n by stable `code` + `params`; fall back to the English `message`.
 * Params that carry a stored UTC timestamp (by name, `DATE_TIME_PARAMS`) are shown as local date
 * and time in the user's language.
 */

import type { TFunction } from "i18next";

import { i18n } from "@/i18n/config";
import { ISSUE_NAMESPACES } from "@/i18n/namespaces";
import { localizeDateTimeParams } from "@/utils/date-params";
import type { OrgValidationIssue } from "@shared/ipc-types";

export type OrgIssueLike = Pick<OrgValidationIssue, "code" | "message"> & {
  params?: Record<string, string> | null;
};

const ISSUE_KEY_PREFIXES = ["governance.issues", "errors", "preview.issues"] as const;
/**
 * Coded issues of the other modules are translated in their own namespaces (`ISSUE_NAMESPACES`,
 * loaded at start-up, so the text is French wherever the refusal surfaces); site-scope refusals
 * (any module) are in the eager `errors` namespace.
 */
const OTHER_NAMESPACE_KEY_PREFIXES = [
  ...ISSUE_NAMESPACES.map((ns) => `${ns}:errors`),
  "errors:scope",
  // GATE_* rule codes (the backend sends them with their parameters: `GATE_X:min_len=10`).
  "errors:gateError",
] as const;

function issueKeys(code: string, orgNs: string): string[] {
  return [
    ...ISSUE_KEY_PREFIXES.map((prefix) => `${orgNs}${prefix}.${code}`),
    ...OTHER_NAMESPACE_KEY_PREFIXES.map((prefix) => `${prefix}.${code}`),
  ];
}

/** The issue's params for display: instants in the user's language and time zone. */
function displayParams(params: Record<string, string> | null | undefined): Record<string, string> {
  // "fr" | "en" (the formatters' locale codes), also when i18next reports a region ("en-US").
  const language = (i18n.resolvedLanguage || i18n.language || "").split("-")[0] ?? "";
  return localizeDateTimeParams(params ?? {}, language);
}

function translateIssueKey(
  code: string,
  params: Record<string, string>,
  t: (key: string, opts: Record<string, unknown>) => string,
  orgNs = "",
): string | null {
  for (const key of issueKeys(code, orgNs)) {
    const translated = t(key, { ...params, defaultValue: "" });
    if (translated && translated !== key && translated.trim() !== "") {
      return translated;
    }
  }
  return null;
}

/** The text of an issue no translation exists for: its backend sentence, but never a GATE code. */
function untranslatedText(issue: OrgIssueLike): string {
  return issue.code.startsWith("GATE_")
    ? String(i18n.t("errors:generic.unexpected"))
    : issue.message;
}

export function formatOrgValidationIssue(issue: OrgIssueLike, t: TFunction<"org">): string {
  const params = displayParams(issue.params);
  const translated = translateIssueKey(issue.code, params, (key, opts) => String(t(key, opts)));
  if (translated) return translated;
  return untranslatedText(issue);
}

/** Same as formatOrgValidationIssue but uses the global i18n instance (stores/toasts). */
export function formatOrgIssueWithI18n(issue: OrgIssueLike): string {
  const params = displayParams(issue.params);
  const translated = translateIssueKey(
    issue.code,
    params,
    (key, opts) => String(i18n.t(key, opts)),
    "org:",
  );
  if (translated) return translated;
  return untranslatedText(issue);
}

export function formatOrgIssuesWithI18n(issues: OrgIssueLike[]): string {
  return issues.map(formatOrgIssueWithI18n).filter(Boolean).join("; ");
}

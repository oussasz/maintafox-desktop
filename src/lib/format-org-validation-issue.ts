/**
 * Localize org validation / mutation issues for end users.
 * Prefer i18n by stable `code` + `params`; fall back to the English `message`.
 */

import type { TFunction } from "i18next";

import { i18n } from "@/i18n/config";
import type { OrgValidationIssue } from "@shared/ipc-types";

export type OrgIssueLike = Pick<OrgValidationIssue, "code" | "message"> & {
  params?: Record<string, string> | null;
};

const ISSUE_KEY_PREFIXES = ["governance.issues", "errors", "preview.issues"] as const;
/** Asset, reference-data, training and reliability commands return coded issues, translated in their own namespaces. */
const OTHER_NAMESPACE_KEY_PREFIXES = [
  "equipment:errors",
  "reference:errors",
  "training:errors",
  "reliability:errors",
  "settings:errors",
] as const;

function issueKeys(code: string, orgNs: string): string[] {
  return [
    ...ISSUE_KEY_PREFIXES.map((prefix) => `${orgNs}${prefix}.${code}`),
    ...OTHER_NAMESPACE_KEY_PREFIXES.map((prefix) => `${prefix}.${code}`),
  ];
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

export function formatOrgValidationIssue(issue: OrgIssueLike, t: TFunction<"org">): string {
  const params = issue.params ?? {};
  const translated = translateIssueKey(issue.code, params, (key, opts) => String(t(key, opts)));
  if (translated) return translated;
  return issue.message;
}

/** Same as formatOrgValidationIssue but uses the global i18n instance (stores/toasts). */
export function formatOrgIssueWithI18n(issue: OrgIssueLike): string {
  const params = issue.params ?? {};
  const translated = translateIssueKey(
    issue.code,
    params,
    (key, opts) => String(i18n.t(key, opts)),
    "org:",
  );
  if (translated) return translated;
  return issue.message;
}

export function formatOrgIssuesWithI18n(issues: OrgIssueLike[]): string {
  return issues.map(formatOrgIssueWithI18n).filter(Boolean).join("; ");
}

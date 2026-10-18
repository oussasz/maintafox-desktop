/**
 * Text of a policy draft test result in the user's language: its code (`POLICY_TEST_*`) with the
 * values it carries; a code this version does not know shows the server's text.
 */
import type { PolicyTestResult } from "@shared/ipc-types";

type Translate = (key: string, options?: Record<string, unknown>) => string;

export const KNOWN_POLICY_TEST_CODES: ReadonlySet<string> = new Set([
  "POLICY_TEST_SCHEMA_INVALID",
  "POLICY_TEST_IN_RANGE",
  "POLICY_TEST_OUT_OF_RANGE",
  "POLICY_TEST_IDLE_WITHIN_ABSOLUTE",
  "POLICY_TEST_IDLE_EXCEEDS_ABSOLUTE",
  "POLICY_TEST_SCHEDULE_EMPTY",
  "POLICY_TEST_SCHEDULE_INVALID",
  "POLICY_TEST_SCHEDULE_VALID",
  "POLICY_TEST_INCLUDE_PHOTOS",
  "POLICY_TEST_PASSWORD_ELSEWHERE",
  "POLICY_TEST_DOMAIN_UNSUPPORTED",
]);

export function policyTestText(result: PolicyTestResult, t: Translate): string {
  if (!KNOWN_POLICY_TEST_CODES.has(result.code)) return result.message;
  const params: Record<string, unknown> = { ...result.params };
  const field = result.params["field"];
  if (field) params["field"] = t(`policy.fields.${field}`, { defaultValue: field });
  if (result.code === "POLICY_TEST_INCLUDE_PHOTOS") {
    params["value"] =
      result.params["value"] === "true" ? t("policy.test.yes") : t("policy.test.no");
  }
  return t(`policy.test.${result.code}`, params);
}

/**
 * What a request's conversion decided about the permit of the new work order, in the user's
 * words: the plant rule that matched (and its permit type), the request's safety flag, no rule
 * matching, or no rule configured at all; plus a warning for published rules that do not read.
 */
import type { PermitFlagOutcome } from "@shared/ipc-types";

type Translate = (key: string, options?: Record<string, unknown>) => string;

export interface PermitFlagMessage {
  title: string;
  description?: string;
  variant: "default" | "destructive";
  /**
   * Stays until dismissed: rules that did not read were not applied, so the work order may lack a
   * permit flag it should have had.
   */
  persistent?: boolean;
}

export function permitFlagMessages(
  outcome: PermitFlagOutcome,
  woCode: string,
  t: Translate,
): PermitFlagMessage[] {
  const out: PermitFlagMessage[] = [];
  if (outcome.reason_code === "PERMIT_RULE_MATCHED" && outcome.rule_code) {
    out.push({
      title: t("woConversion.permit.required", { code: woCode }),
      description: outcome.permit_type_code
        ? t("woConversion.permit.byRule", {
            rule: outcome.rule_code,
            permitType: outcome.permit_type_code,
          })
        : t("woConversion.permit.byRuleNoType", { rule: outcome.rule_code }),
      variant: "default",
    });
  } else if (outcome.requires_permit && outcome.safety_flag) {
    out.push({
      title: t("woConversion.permit.required", { code: woCode }),
      description: t("woConversion.permit.bySafetyFlag"),
      variant: "default",
    });
  } else if (outcome.reason_code === "PERMIT_RULES_NONE_CONFIGURED") {
    out.push({
      title: t("woConversion.permit.noneConfigured"),
      description: t("woConversion.permit.noneConfiguredHint"),
      variant: "default",
    });
  } else if (outcome.reason_code === "PERMIT_RULES_NO_MATCH") {
    out.push({ title: t("woConversion.permit.noMatch", { code: woCode }), variant: "default" });
  }
  if (outcome.unreadable_rule_codes.length > 0) {
    out.push({
      title: t("woConversion.permit.unreadableTitle"),
      description: t("woConversion.permit.unreadable", {
        code: woCode,
        codes: outcome.unreadable_rule_codes.join(", "),
      }),
      variant: "destructive",
      persistent: true,
    });
  }
  return out;
}

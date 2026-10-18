/**
 * The two named "open OT" rules (owner decision: keep both, name them). Every figure counting
 * open work orders carries the code of the rule it counts (backend `wo::backlog::RULE_*`); the
 * figure shows that rule's name and its list link applies the same rule as a list filter.
 *
 * - `open_work_to_do` — "Open (work to do)": draft to on hold (`OPEN_STATUS_SQL`, list filter
 *   `open_only`, link `scope=open`).
 * - `not_yet_closed` — "Not yet closed": every status but closed or cancelled (`NOT_CLOSED_SQL`,
 *   list filter `not_closed_only`, link `scope=not_closed`).
 */
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import type { ListScope } from "@/lib/list-links";

export const WO_OPEN_RULES = ["open_work_to_do", "not_yet_closed"] as const;
export type WoOpenRule = (typeof WO_OPEN_RULES)[number];

/** The rule code sent by the server, or null for a code this version does not know. */
export function woOpenRule(code: string | null | undefined): WoOpenRule | null {
  return (WO_OPEN_RULES as readonly string[]).includes(code ?? "") ? (code as WoOpenRule) : null;
}

/** List scope applying the rule (`?scope=` of the work-order list link). */
export function woOpenRuleScope(rule: WoOpenRule): Extract<ListScope, "open" | "not_closed"> {
  return rule === "open_work_to_do" ? "open" : "not_closed";
}

/** i18n key (common namespace) of the rule's name. */
export function woOpenRuleLabelKey(rule: WoOpenRule): string {
  return `woOpenRule.${rule}`;
}

export interface WoOpenRuleText {
  rule: WoOpenRule;
  /** Rule name shown next to the figure ("Open (work to do)" / "Not yet closed"). */
  label: string;
  /** One sentence saying what the rule counts (tooltip). */
  hint: string;
  /** List scope applying the same rule. */
  scope: Extract<ListScope, "open" | "not_closed">;
  /** Label of a work-order count under this rule ("Work orders — Not yet closed"). */
  figure: string;
}

/** Name, explanation and list scope of a figure's rule code; null for an unknown code. */
export function useWoOpenRuleText(): (code: string | null | undefined) => WoOpenRuleText | null {
  const { t } = useTranslation("common");
  return useCallback(
    (code) => {
      const rule = woOpenRule(code);
      if (!rule) return null;
      return {
        rule,
        label: String(t(woOpenRuleLabelKey(rule))),
        hint: String(t(`woOpenRule.hint.${rule}`)),
        scope: woOpenRuleScope(rule),
        figure: String(t("woOpenRule.figure", { rule: String(t(woOpenRuleLabelKey(rule))) })),
      };
    },
    [t],
  );
}

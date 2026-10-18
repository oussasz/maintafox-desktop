/**
 * What a conversion decided about the new work order's permit: the rule that matched, the
 * request's safety flag, no rule matching, no rule configured, and unreadable rules.
 */
import { describe, expect, it } from "vitest";

import enDi from "@/i18n/locale-data/en/di.json";
import type { PermitFlagOutcome } from "@shared/ipc-types";

import { permitFlagMessages } from "../permit-flag-text";

function lookup(key: string): string {
  const value = key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], enDi);
  if (typeof value !== "string") throw new Error(`missing di key ${key}`);
  return value;
}
const t = (key: string, opts?: Record<string, unknown>) =>
  lookup(key).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));

function outcome(overrides: Partial<PermitFlagOutcome>): PermitFlagOutcome {
  return {
    requires_permit: false,
    reason_code: "PERMIT_RULES_NO_MATCH",
    rule_code: null,
    permit_type_code: null,
    safety_flag: false,
    unreadable_rule_codes: [],
    ...overrides,
  };
}

describe("permitFlagMessages", () => {
  it("says plainly that no permit rule is configured", () => {
    const [m] = permitFlagMessages(
      outcome({ reason_code: "PERMIT_RULES_NONE_CONFIGURED" }),
      "OT-1",
      t,
    );
    expect(m?.title).toBe("No permit requirement rule is configured");
    expect(m?.description).toContain("Permit requirement rules");
  });

  it("names the safety flag when no rule matched but the request was a safety risk", () => {
    const [m] = permitFlagMessages(
      outcome({ requires_permit: true, safety_flag: true }),
      "OT-1",
      t,
    );
    expect(m?.title).toBe("Permit required for OT-1");
    expect(m?.description).toContain("safety risk");
  });

  it("says no rule applies when rules exist but none matches", () => {
    expect(permitFlagMessages(outcome({}), "OT-1", t)).toEqual([
      { title: "No permit requirement rule applies to OT-1.", variant: "default" },
    ]);
  });

  it("warns about rules that do not read, as an error", () => {
    const messages = permitFlagMessages(
      outcome({ unreadable_rule_codes: ["OLD_1", "OLD_2"] }),
      "OT-1",
      t,
    );
    expect(messages[1]).toEqual({
      title: "Unreadable permit rules",
      description:
        "These rules could not be read and were not applied to OT-1: OLD_1, OLD_2. Check whether OT-1 needs a permit, then fix the rules in Reference data › Permit requirement rules.",
      variant: "destructive",
      persistent: true,
    });
  });
});

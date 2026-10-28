/**
 * RCM decision diagram: which yes/no questions apply for the current answers (EN 60300-3-11 /
 * MSG-3 style logic). The study wizard's RCM step asks them in this order.
 */
import type { RcmAnswers } from "@shared/ipc-types";

type BoolKey = Exclude<keyof RcmAnswers, "weibull_beta">;

/** Questions shown for the current answers, in the order of the decision diagram. */
export function visibleQuestions(a: RcmAnswers): BoolKey[] {
  const q: BoolKey[] = ["failure_evident"];
  if (a.failure_evident) {
    q.push("safety_or_environmental");
    if (!a.safety_or_environmental) q.push("operational_impact");
  } else {
    q.push("multiple_failure_affects_safety");
  }
  q.push("on_condition_feasible", "scheduled_restoration_feasible", "scheduled_discard_feasible");
  if (!a.failure_evident) q.push("failure_finding_feasible");
  if (a.failure_evident && a.safety_or_environmental) q.push("combination_feasible");
  return q;
}

import { describe, expect, it } from "vitest";

import { visibleQuestions } from "../RamsLoopDialogs";

const base = {
  failure_evident: true,
  safety_or_environmental: false,
  operational_impact: true,
  multiple_failure_affects_safety: false,
  on_condition_feasible: false,
  scheduled_restoration_feasible: false,
  scheduled_discard_feasible: false,
  failure_finding_feasible: false,
  combination_feasible: false,
  weibull_beta: null,
};

describe("visibleQuestions", () => {
  it("follows the decision diagram branches", () => {
    expect(visibleQuestions(base)).toEqual([
      "failure_evident",
      "safety_or_environmental",
      "operational_impact",
      "on_condition_feasible",
      "scheduled_restoration_feasible",
      "scheduled_discard_feasible",
    ]);
    expect(visibleQuestions({ ...base, failure_evident: false })).toContain(
      "failure_finding_feasible",
    );
    expect(visibleQuestions({ ...base, safety_or_environmental: true })).toContain(
      "combination_feasible",
    );
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { RcmDecisionDialog, visibleQuestions } from "../RamsLoopDialogs";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const svc = vi.hoisted(() => ({
  applyRcmDecisionLogic: vi.fn(),
  computeFmecaCriticality: vi.fn(),
  createPmRevisionFromRamsSource: vi.fn(),
  createWoFromRamsSource: vi.fn(),
  getFmecaCriticalityEvidence: vi.fn(),
  listRcmDecisionLogic: vi.fn(),
  previewRcmRecommendation: vi.fn(),
}));
vi.mock("@/services/reliability-service", () => svc);
vi.mock("@/services/pm-service", () => ({ listPmPlans: vi.fn().mockResolvedValue([]) }));
vi.mock("@/services/wo-service", () => ({ listWorkOrderTypes: vi.fn().mockResolvedValue([]) }));

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

describe("RcmDecisionDialog", () => {
  it("previews the recommendation and records the decision", async () => {
    svc.listRcmDecisionLogic.mockResolvedValue([
      {
        decision_id: 5,
        row_version: 3,
        selected_tactic: "run_to_failure",
        consequence_category: "",
        justification: "",
        recommended_tactic: null,
        answers: null,
        path: [],
        linked_pm_plan_id: null,
        linked_pm_plan_version_id: null,
        pm_plan_code: null,
        pm_version_no: null,
        linked_work_order_id: null,
        work_order_code: null,
      },
    ]);
    svc.previewRcmRecommendation.mockResolvedValue({
      consequence_category: "operational",
      recommended_tactic: "condition_based",
      redesign_compulsory: false,
      path: ["evident:yes", "on_condition:yes"],
    });
    svc.applyRcmDecisionLogic.mockImplementation(async (input: { decision_id: number }) => ({
      decision_id: input.decision_id,
      row_version: 4,
      selected_tactic: "condition_based",
      consequence_category: "operational",
      justification: "",
      recommended_tactic: "condition_based",
      answers: base,
      path: ["evident:yes"],
      linked_pm_plan_id: null,
      linked_pm_plan_version_id: null,
      pm_plan_code: null,
      pm_version_no: null,
      linked_work_order_id: null,
      work_order_code: null,
    }));
    render(<RcmDecisionDialog studyId={1} decisionId={5} onClose={vi.fn()} onSaved={vi.fn()} />);
    expect(await screen.findByTestId("rcm-preview")).toHaveTextContent(
      "rcmLoop.tactics.condition_based",
    );
    fireEvent.click(screen.getByRole("button", { name: "rcmLoop.apply" }));
    await waitFor(() =>
      expect(svc.applyRcmDecisionLogic).toHaveBeenCalledWith(
        expect.objectContaining({
          decision_id: 5,
          expected_row_version: 3,
          selected_tactic: "condition_based",
        }),
      ),
    );
  });
});

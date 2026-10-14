import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import frPlanning from "@/i18n/locale-data/fr/planning.json";
import type { CandidateConflictSummary, ScheduleCandidate } from "@shared/ipc-types";

import { PlanningBacklogRail } from "../PlanningBacklogRail";
import { candidateSourcePath } from "../planning-model";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPlanning,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      (
        lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
      ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? "")),
    i18n: { language: "en" },
  }),
}));

/** Conflict types emitted by `planning/queries.rs` (EvaluatedConflict.conflict_type). */
const BACKEND_BLOCKER_CODES = [
  "missing_critical_part",
  "no_qualified_technician",
  "permit_not_ready",
  "prerequisite_incomplete",
  "locked_window",
  "double_booking",
];

function candidate(overrides: Partial<ScheduleCandidate> = {}): ScheduleCandidate {
  return {
    id: 1,
    source_type: "work_order",
    source_id: 44,
    source_di_id: null,
    readiness_status: "blocked",
    readiness_score: 40,
    skill_match_score: null,
    estimated_assignment_risk: null,
    priority_label: null,
    source_code: "OT-0044",
    source_title: "Pump overhaul",
    ...overrides,
  } as ScheduleCandidate;
}

function LocationProbe() {
  const loc = useLocation();
  return <div data-testid="location">{`${loc.pathname}${loc.search}`}</div>;
}

describe("planning blockers", () => {
  it("has FR and EN labels for every blocker code the backend emits", () => {
    for (const code of BACKEND_BLOCKER_CODES) {
      expect(enPlanning.blockers).toHaveProperty(code);
      expect(frPlanning.blockers).toHaveProperty(code);
    }
  });

  it("links planning sources to the screen that shows them", () => {
    expect(candidateSourcePath(candidate())).toBe("/work-orders?openWo=44");
    expect(
      candidateSourcePath(candidate({ source_type: "inspection_follow_up", source_di_id: 9 })),
    ).toBe("/requests?openDi=9");
    expect(candidateSourcePath(candidate({ source_type: "pm_occurrence", source_id: 3 }))).toBe(
      null,
    );
  });

  it("shows translated blockers and opens the OT from the blocked list", () => {
    const summary: CandidateConflictSummary[] = [
      { candidate_id: 1, blocker_codes: ["missing_critical_part", "permit_not_ready"] },
    ] as CandidateConflictSummary[];
    render(
      <MemoryRouter initialEntries={["/planning"]}>
        <Routes>
          <Route
            path="*"
            element={
              <>
                <PlanningBacklogRail
                  ready={[]}
                  blocked={[candidate()]}
                  conflictSummary={summary}
                  selectedCandidateId={null}
                  onSelectReady={vi.fn()}
                />
                <LocationProbe />
              </>
            }
          />
        </Routes>
      </MemoryRouter>,
    );
    fireEvent.mouseDown(screen.getByRole("tab", { name: /Blocked|blocked/ }));
    fireEvent.click(screen.getByRole("tab", { name: /Blocked|blocked/ }));
    expect(screen.getByText(enPlanning.blockers.missing_critical_part)).toBeInTheDocument();
    expect(screen.getByText(enPlanning.blockers.permit_not_ready)).toBeInTheDocument();
    expect(screen.queryByText("missing_critical_part")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Open OT-0044" }));
    expect(screen.getByTestId("location")).toHaveTextContent("/work-orders?openWo=44");
  });
});

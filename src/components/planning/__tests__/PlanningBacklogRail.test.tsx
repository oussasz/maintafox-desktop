import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import frPlanning from "@/i18n/locale-data/fr/planning.json";
import type { CandidateConflictSummary, ScheduleCandidate } from "@shared/ipc-types";

import { PlanningBacklogRail } from "../PlanningBacklogRail";
import {
  candidateSourcePath,
  conflictReasonLinks,
  conflictReasonText,
  parseConflictReasons,
  timelineItemTooltip,
} from "../planning-model";

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
/** Reason codes emitted by `planning/queries.rs` (EvaluatedConflict.reason_code). */
const BACKEND_REASON_CODES = [
  "NOT_ESTIMATED",
  "PARTS_NOT_RESERVED",
  "SKILL_GAP",
  "ASSIGNMENT_REQUIRED",
  "PERMIT_REQUIRED",
  "PREREQUISITE_BLOCKED",
  "NO_FEASIBLE_WINDOW",
  "ASSIGNEE_UNAVAILABLE",
  "PARTS_SHORT",
  "PART_NOT_IN_CATALOGUE",
  "PART_QUANTITY_MISSING",
  "PERMIT_TO_REQUEST",
  "PERMIT_IN_PREPARATION",
  "PERMIT_ON_HOLD",
  "PERMIT_EXPIRES_BEFORE_WORK",
  "LOTO_NOT_VERIFIED",
  "PERMIT_TYPE_MISSING",
  "LOCATION_UNKNOWN",
];

const BACKEND_BLOCKER_CODES = [
  "missing_critical_part",
  "no_qualified_technician",
  "permit_not_ready",
  "prerequisite_incomplete",
  "locked_window",
  "double_booking",
  "location_unknown",
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

describe("backlog card shutdown badge", () => {
  function renderReady(c: ScheduleCandidate) {
    render(
      <MemoryRouter>
        <PlanningBacklogRail
          ready={[c]}
          blocked={[]}
          conflictSummary={[]}
          selectedCandidateId={null}
          onSelectReady={vi.fn()}
        />
      </MemoryRouter>,
    );
  }

  it("shows the shutdown badge when the job needs the equipment stopped", () => {
    renderReady(candidate({ readiness_status: "ready", shutdown_requirement: "required" }));
    expect(screen.getByText("shutdownRequirement.badge")).toBeInTheDocument();
  });

  it("shows no shutdown badge when the job does not need a shutdown", () => {
    renderReady(candidate({ readiness_status: "ready", shutdown_requirement: null }));
    expect(screen.queryByText("shutdownRequirement.badge")).toBeNull();
  });
});

const tPlanning = ((key: string, opts?: Record<string, unknown>) =>
  (
    lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""))) as unknown as Parameters<
  typeof conflictReasonText
>[1];

describe("conflict reasons", () => {
  it("has FR and EN text for every reason code the backend emits", () => {
    for (const code of BACKEND_REASON_CODES) {
      expect(enPlanning.reasons).toHaveProperty(code);
      expect(frPlanning.reasons).toHaveProperty(code);
    }
  });

  it("explains a reason with its details, lists joined", () => {
    expect(
      conflictReasonText(
        {
          conflict_type: "not_estimated",
          reason_code: "NOT_ESTIMATED",
          severity: "high",
          details_json: JSON.stringify({ missing: ["duration", "crew_size"] }),
        },
        tPlanning,
      ),
    ).toBe("Not estimated: duration, crew size missing.");
    expect(
      conflictReasonText(
        {
          conflict_type: "no_qualified_technician",
          reason_code: "SKILL_GAP",
          severity: "high",
          details_json: JSON.stringify({
            person: "Amine Haddad",
            gaps: ["Habilitation B2", "CACES"],
          }),
        },
        tPlanning,
      ),
    ).toBe("Amine Haddad lacks a qualification this job asks for: Habilitation B2, CACES.");
  });

  it("falls back to the conflict label for an unknown reason or unreadable details", () => {
    expect(
      conflictReasonText(
        {
          conflict_type: "permit_not_ready",
          reason_code: "SOMETHING_NEW",
          severity: "medium",
          details_json: "{not json",
        },
        tPlanning,
      ),
    ).toBe(enPlanning.blockers.permit_not_ready);
  });

  it("reads a commitment's reasons and puts them under the bar's title", () => {
    const reasons = parseConflictReasons(
      JSON.stringify([
        {
          conflict_type: "prerequisite_incomplete",
          reason_code: "PREREQUISITE_BLOCKED",
          severity: "high",
          details_json: null,
        },
        { nonsense: true },
      ]),
    );
    expect(reasons).toHaveLength(1);
    expect(parseConflictReasons("oops")).toEqual([]);
    expect(parseConflictReasons(null)).toEqual([]);
    expect(timelineItemTooltip({ title: "OT-0044", conflictReasons: reasons }, tPlanning)).toBe(
      `OT-0044\n• ${enPlanning.reasons.PREREQUISITE_BLOCKED}`,
    );
    expect(timelineItemTooltip({ title: "OT-0045" }, tPlanning)).toBe("OT-0045");
  });

  it("shows why a card is blocked", () => {
    const summary: CandidateConflictSummary[] = [
      {
        candidate_id: 1,
        blocker_codes: ["locked_window"],
        blocker_dimensions: ["windows"],
        readiness_status: "blocked",
        readiness_score: 80,
        reasons: [
          {
            conflict_type: "locked_window",
            reason_code: "NO_FEASIBLE_WINDOW",
            severity: "medium",
            details_json: null,
          },
        ],
      },
    ];
    render(
      <MemoryRouter>
        <PlanningBacklogRail
          ready={[]}
          blocked={[candidate()]}
          conflictSummary={summary}
          selectedCandidateId={null}
          onSelectReady={vi.fn()}
        />
      </MemoryRouter>,
    );
    fireEvent.mouseDown(screen.getByRole("tab", { name: /Blocked|blocked/ }));
    fireEvent.click(screen.getByRole("tab", { name: /Blocked|blocked/ }));
    expect(screen.getByText(enPlanning.reasons.NO_FEASIBLE_WINDOW)).toBeInTheDocument();
    expect(screen.getByText(enPlanning.blockers.locked_window)).toBeInTheDocument();
  });
});

describe("blocking and warning reasons", () => {
  const short = {
    conflict_type: "missing_critical_part",
    reason_code: "PARTS_SHORT",
    severity: "blocking",
    details_json: JSON.stringify({
      articles: ["PLN-BRG"],
      lines: [{ article: "PLN-BRG", article_id: 3, needed: 2, available: 1, short: 1 }],
    }),
  };
  const permit = {
    conflict_type: "permit_not_ready",
    reason_code: "PERMIT_IN_PREPARATION",
    severity: "warning",
    details_json: JSON.stringify({ permit_id: 7, permit_code: "PTW-7", status: "draft" }),
  };

  it("leads each reason to its permit, article or windows", () => {
    expect(conflictReasonLinks(short)).toEqual([
      { kind: "article", path: "/inventory?articleId=3", code: "PLN-BRG" },
    ]);
    expect(conflictReasonLinks(permit)).toEqual([{ kind: "permit", path: "/permits?permit=7" }]);
    expect(
      conflictReasonLinks({
        conflict_type: "permit_not_ready",
        reason_code: "PERMIT_TO_REQUEST",
        severity: "warning",
        details_json: JSON.stringify({ on_work_order: true, work_order_id: 44 }),
      }),
    ).toEqual([{ kind: "requestPermit", path: "/permits?createForWo=44" }]);
    expect(
      conflictReasonLinks({
        conflict_type: "locked_window",
        reason_code: "NO_FEASIBLE_WINDOW",
        severity: "blocking",
        details_json: null,
      }),
    ).toEqual([{ kind: "windows" }]);
  });

  it("shows blocking reasons first with a lock, warnings in amber, and opens what they are about", () => {
    const onOpenWindows = vi.fn();
    const summary: CandidateConflictSummary[] = [
      {
        candidate_id: 1,
        blocker_codes: ["permit_not_ready", "missing_critical_part"],
        blocker_dimensions: ["parts", "permits"],
        readiness_status: "blocked",
        readiness_score: 70,
        reasons: [permit, short],
      },
    ];
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
                  onOpenWindows={onOpenWindows}
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
    const items = screen.getAllByRole("listitem");
    expect(items.map((li) => li.getAttribute("data-severity"))).toEqual(["blocking", "warning"]);
    expect(screen.getByRole("img", { name: enPlanning.reasonUi.blocking })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: enPlanning.reasonUi.warning })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open article PLN-BRG" }));
    expect(screen.getByTestId("location")).toHaveTextContent("/inventory?articleId=3");
    fireEvent.click(screen.getByRole("button", { name: enPlanning.reasonUi.openPermit }));
    expect(screen.getByTestId("location")).toHaveTextContent("/permits?permit=7");
  });
});

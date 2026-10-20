/**
 * Absences on the planning board (decision register 2026-10-19, Q2): a planner without per.view
 * receives absences without their type and sees a neutral tag only — "absent — critical for
 * planning" or "absent" — drawn critical or as a plain block; a planner with per.view keeps the
 * type's name and colour from the plant's absence list.
 */
import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import frPlanning from "@/i18n/locale-data/fr/planning.json";
import type { PlanningGanttSnapshot } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const lookup = vi.hoisted(() => ({
  fn: (code: string): { text: string; tag: string | null } | null =>
    code === "medical" ? { text: "Arrêt médical", tag: "critical" } : null,
}));
vi.mock("@/components/personnel/use-personnel-lists", () => ({
  useAbsenceTypeLookup: () => lookup.fn,
}));

type CapturedItem = { id: string | number; title?: unknown; state?: string };
const captured = vi.hoisted(() => ({ items: [] as CapturedItem[] }));
vi.mock("@/components/timeline/ResourceTimeline", () => ({
  ResourceTimeline: (props: { items: CapturedItem[] }) => {
    captured.items = props.items;
    return null;
  },
}));

import { PlanningBoard } from "../PlanningBoard";

function snapshot(intervals: unknown[]): PlanningGanttSnapshot {
  return {
    period_start: "2026-10-19T00:00:00Z",
    period_end: "2026-10-26T00:00:00Z",
    commitments: [],
    locked_windows: [],
    capacity: [],
    assignee_lanes: [
      {
        personnel_id: 7,
        full_name: "Amine",
        blocked_intervals_json: JSON.stringify(intervals),
        commitments_json: "[]",
      },
    ],
    crew_lanes: [],
  } as unknown as PlanningGanttSnapshot;
}

function renderBoard(gantt: PlanningGanttSnapshot) {
  render(
    <PlanningBoard
      gantt={gantt}
      commitments={[]}
      capacityLoad={[]}
      lockedWindows={[]}
      visibleStart={Date.parse("2026-10-19T00:00:00Z")}
      visibleEnd={Date.parse("2026-10-26T00:00:00Z")}
      horizonStart={Date.parse("2026-10-19T00:00:00Z")}
      horizonEnd={Date.parse("2026-10-26T00:00:00Z")}
      onVisibleTimeChange={vi.fn()}
      selectedCommitmentId={null}
      onSelectCommitment={vi.fn()}
      onReschedule={vi.fn()}
      laneMode="technician"
    />,
  );
  return captured.items.map((i) => [i.title, i.state]);
}

beforeEach(() => {
  captured.items = [];
});

describe("Absences on the planning board", () => {
  it("without per.view, shows only the neutral tag: critical for planning, or absent", () => {
    // What the server sends a planner without per.view: no type, only when and the tag.
    const rows = renderBoard(
      snapshot([
        { start_at: "2026-10-19T08:00:00Z", end_at: "2026-10-19T16:00:00Z", critical: true },
        { start_at: "2026-10-20T08:00:00Z", end_at: "2026-10-20T16:00:00Z", critical: false },
      ]),
    );
    expect(rows).toEqual([
      ["board.absenceTag.critical", "critical"],
      ["board.absenceTag.plain", "blocked"],
    ]);
  });

  it("with per.view, keeps the type's name and its tag from the plant's list", () => {
    const rows = renderBoard(
      snapshot([
        {
          start_at: "2026-10-19T08:00:00Z",
          end_at: "2026-10-19T16:00:00Z",
          block_type: "medical",
          critical: false,
        },
      ]),
    );
    expect(rows).toEqual([["Arrêt médical", "critical"]]);
  });

  it("names the two tags in English and French", () => {
    expect(enPlanning.board.absenceTag).toEqual({
      critical: "Absent — critical for planning",
      plain: "Absent",
    });
    expect(frPlanning.board.absenceTag).toEqual({
      critical: "Absent — critique pour la planification",
      plain: "Absent",
    });
  });
});

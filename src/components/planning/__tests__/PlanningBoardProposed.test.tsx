/**
 * Planned OTs without a commitment are drawn on the board as read-only, dashed "to commit" bars:
 * not movable, a click names the OT, and the header counter says "committed N · to commit M".
 */
import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import frPlanning from "@/i18n/locale-data/fr/planning.json";
import type { PlanningGanttSnapshot } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

vi.mock("@/components/personnel/use-personnel-lists", () => ({
  useAbsenceTypeLookup: () => () => null,
}));

type CapturedItem = {
  id: string | number;
  group: string | number;
  className?: string;
  canMove?: boolean;
};
type CapturedProps = {
  items: CapturedItem[];
  onItemSelect?: (itemId: string | number) => void;
};
const captured = vi.hoisted(() => ({ props: null as CapturedProps | null }));
vi.mock("@/components/timeline/ResourceTimeline", () => ({
  ResourceTimeline: (props: CapturedProps) => {
    captured.props = props;
    return null;
  },
}));

import { PlanningBoard } from "../PlanningBoard";

const gantt = {
  period_start: "2026-10-01T00:00:00Z",
  period_end: "2026-10-08T00:00:00Z",
  commitments: [],
  locked_windows: [],
  capacity: [],
  assignee_lanes: [],
  crew_lanes: [{ team_id: 3, team_code: "MEC", team_name: "Mécanique", commitments_json: "[]" }],
  proposed_work_orders: [
    {
      work_order_id: 42,
      code: "WO-0042",
      title: "Changer le roulement",
      planned_start: "2026-10-01T08:00:00Z",
      planned_end: "2026-10-01T10:00:00Z",
      assigned_team_id: 3,
      assigned_team_name: "Mécanique",
      equipment_label: "Pompe P1",
    },
  ],
} as unknown as PlanningGanttSnapshot;

function renderBoard(laneMode: "crew" | "technician", onSelectProposed = vi.fn()) {
  render(
    <PlanningBoard
      gantt={gantt}
      commitments={[]}
      capacityLoad={[]}
      lockedWindows={[]}
      visibleStart={Date.parse("2026-10-01T00:00:00Z")}
      visibleEnd={Date.parse("2026-10-08T00:00:00Z")}
      horizonStart={Date.parse("2026-10-01T00:00:00Z")}
      horizonEnd={Date.parse("2026-10-08T00:00:00Z")}
      onVisibleTimeChange={vi.fn()}
      selectedCommitmentId={null}
      onSelectCommitment={vi.fn()}
      onSelectProposed={onSelectProposed}
      onReschedule={vi.fn()}
      canDrag
      laneMode={laneMode}
    />,
  );
  return onSelectProposed;
}

beforeEach(() => {
  captured.props = null;
});

describe("Planned OTs without commitment on the board", () => {
  it("draws a dashed, non-movable bar in the crew lane and reports its click", () => {
    const onSelectProposed = renderBoard("crew");
    const bar = captured.props?.items.find((i) => i.id === "prop-42");
    expect(bar).toMatchObject({
      group: "team-3",
      className: "mf-rt-item--proposed",
      canMove: false,
    });
    captured.props?.onItemSelect?.("prop-42");
    expect(onSelectProposed).toHaveBeenCalledWith(42);
  });

  it("puts the bar in the unassigned lane when single-crew technician lanes are shown", () => {
    renderBoard("technician");
    const bar = captured.props?.items.find((i) => i.id === "prop-42");
    expect(bar?.group).toBe("unassigned");
  });

  it("has the counter and bar texts in both languages", () => {
    expect(frPlanning.page.counter).toBe("engagés {{committed}} · à engager {{proposed}}");
    expect(enPlanning.page.counter).toContain("{{proposed}}");
    expect(Object.keys(frPlanning.proposed).sort()).toEqual(
      Object.keys(enPlanning.proposed).sort(),
    );
    expect(frPlanning.proposed.label).toBe("À engager");
  });
});

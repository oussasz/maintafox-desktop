import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import type { WorkOrder } from "@shared/ipc-types";

import { WoShiftSelect } from "../WoShiftSelect";
import { formatWoShift } from "../wo-shift-label";
import { plannedOutsideWindow, shiftSaveFields } from "../wo-shift-plan";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}:${JSON.stringify(opts)}` : key,
    i18n: { language: "fr" },
  }),
}));

const svc = vi.hoisted(() => ({ listWoShiftOptions: vi.fn() }));
vi.mock("@/services/wo-service", () => svc);

const t = (key: string) => `t(${key})`;

function wo(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: 7,
    assigned_team_id: 3,
    shift: null,
    shift_class_version_id: null,
    shift_class_label: null,
    shift_start: null,
    shift_end: null,
    ...overrides,
  } as WorkOrder;
}

const DAY = {
  class_version_id: 11,
  schedule_class_id: 4,
  class_label: "Poste jour",
  shift_start: "06:00",
  shift_end: "14:00",
  break_minutes: 30,
  net_hours: 7.5,
  members: 2,
  members_available: 2,
  starts_previous_day: false,
};

function options(list: (typeof DAY)[], reason: string | null = null, team = 3) {
  return { team_id: team, day: "2026-04-13", options: list, reason };
}

function renderSelect(
  w: WorkOrder,
  props: {
    start?: string | null;
    end?: string | null;
    teamId?: number | null;
    value?: string;
  } = {},
) {
  return render(
    <MemoryRouter>
      <WoShiftSelect
        wo={w}
        plannedStart={props.start === undefined ? "2026-04-13T08:00" : props.start}
        plannedEnd={props.end ?? null}
        teamId={props.teamId ?? null}
        value={props.value ?? ""}
        onChange={() => undefined}
      />
    </MemoryRouter>,
  );
}

describe("formatWoShift", () => {
  it("shows the window the work order was planned with, else the old code, else nothing", () => {
    expect(
      formatWoShift(
        wo({ shift_class_label: "Poste jour", shift_start: "06:00", shift_end: "14:00" }),
        t,
      ),
    ).toBe("Poste jour 06:00–14:00");
    expect(formatWoShift(wo({ shift: "night" }), t)).toBe("t(shift.night)");
    expect(formatWoShift(wo(), t)).toBeNull();
  });
});

describe("shiftSaveFields — what a planning save or an assignment sends", () => {
  const planned = wo({ shift_class_version_id: 11, shift_start: "06:00", shift_end: "14:00" });

  it("sends nothing when the planner did not change the shift (hours edit, date move, team change)", () => {
    expect(shiftSaveFields(false, "11", planned)).toEqual({});
    expect(shiftSaveFields(false, "", planned)).toEqual({});
  });

  it("keeps an old shift code on a save that does not touch the shift", () => {
    expect(shiftSaveFields(false, "", wo({ shift: "morning" }))).toEqual({});
  });

  it("sends the planner's choice: a window, or no shift", () => {
    expect(shiftSaveFields(true, "12", planned)).toEqual({ shift_class_version_id: 12 });
    expect(shiftSaveFields(true, "", planned)).toEqual({ clear_shift: true });
    expect(shiftSaveFields(true, "", wo({ shift: "morning" }))).toEqual({ clear_shift: true });
    expect(shiftSaveFields(true, "", wo())).toEqual({});
  });
});

describe("plannedOutsideWindow — a warning, never a refusal", () => {
  it("checks the planned start and end against the window", () => {
    expect(plannedOutsideWindow("2026-04-13T08:00", "2026-04-13T12:00", DAY)).toBe(false);
    expect(plannedOutsideWindow("2026-04-13T05:00", "2026-04-13T08:00", DAY)).toBe(true);
    expect(plannedOutsideWindow("2026-04-13T13:00", "2026-04-13T15:00", DAY)).toBe(true);
  });

  it("follows a night shift past midnight, and the previous night's shift", () => {
    const night = { shift_start: "22:00", shift_end: "06:00" };
    expect(plannedOutsideWindow("2026-04-13T23:00", "2026-04-14T04:00", night)).toBe(false);
    expect(plannedOutsideWindow("2026-04-13T23:00", "2026-04-14T07:00", night)).toBe(true);
    expect(
      plannedOutsideWindow("2026-04-14T02:00", "2026-04-14T05:00", {
        ...night,
        starts_previous_day: true,
      }),
    ).toBe(false);
  });
});

describe("WoShiftSelect", () => {
  it("asks the shifts at the planned start as typed", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([DAY]));
    renderSelect(wo());
    await waitFor(() =>
      expect(svc.listWoShiftOptions).toHaveBeenCalledWith(7, "2026-04-13T08:00", null),
    );
    expect(screen.queryByText("planning.shiftNoShift")).toBeNull();
  });

  it("offers the shifts of the team being assigned", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([], "no_shift", 9));
    renderSelect(wo(), { teamId: 9 });
    await waitFor(() =>
      expect(svc.listWoShiftOptions).toHaveBeenCalledWith(7, "2026-04-13T08:00", 9),
    );
    expect(screen.getByText("planning.shiftForNewTeam")).toBeTruthy();
  });

  it("explains why nothing is offered and links to the team", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([], "no_shift"));
    renderSelect(wo());
    expect(await screen.findByText("planning.shiftNoShift")).toBeTruthy();
    expect(screen.getByRole("link", { name: "planning.shiftOpenTeam" }).getAttribute("href")).toBe(
      "/personnel?tab=teams&team=3",
    );
  });

  it("asks for a team, and shows an old shift code still to replace", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([], "no_team"));
    renderSelect(wo({ assigned_team_id: null, shift: "morning" }));
    expect(await screen.findByText("planning.shiftNoTeam")).toBeTruthy();
    expect(screen.getByText(/planning\.shiftLegacy/)).toBeTruthy();
  });

  it("warns when the planned hours fall outside the chosen shift", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([DAY]));
    renderSelect(wo(), { start: "2026-04-13T15:00", end: "2026-04-13T17:00", value: "11" });
    expect(await screen.findByText("planning.shiftOutsideWindow")).toBeTruthy();
  });

  it("tells the planner the timetable changed since planning", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([{ ...DAY, shift_start: "07:00" }]));
    renderSelect(
      wo({
        shift_class_version_id: 11,
        shift_class_label: "Poste jour",
        shift_start: "06:00",
        shift_end: "14:00",
      }),
      { value: "11" },
    );
    const notice = await screen.findByText(/planning\.shiftTimetableChanged/);
    expect(notice.textContent).toContain('"planned":"06:00–14:00"');
    expect(notice.textContent).toContain('"now":"07:00–14:00"');
  });

  it("drops the previous team's shifts while the new ones load", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce(options([], "no_shift"));
    const view = renderSelect(wo());
    expect(await screen.findByText("planning.shiftNoShift")).toBeTruthy();
    let resolve: (v: unknown) => void = () => undefined;
    svc.listWoShiftOptions.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    view.rerender(
      <MemoryRouter>
        <WoShiftSelect
          wo={wo()}
          plannedStart="2026-04-13T08:00"
          teamId={9}
          value=""
          onChange={() => undefined}
        />
      </MemoryRouter>,
    );
    await waitFor(() => expect(screen.queryByText("planning.shiftNoShift")).toBeNull());
    resolve(options([DAY], null, 9));
  });
});

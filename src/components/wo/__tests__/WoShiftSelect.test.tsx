import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import type { WorkOrder } from "@shared/ipc-types";

import { WoShiftSelect } from "../WoShiftSelect";
import { formatWoShift } from "../wo-shift-label";

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

function renderSelect(w: WorkOrder, plannedDay: string | null = "2026-04-13") {
  return render(
    <MemoryRouter>
      <WoShiftSelect wo={w} plannedDay={plannedDay} value="" onChange={() => undefined} />
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

describe("WoShiftSelect", () => {
  it("asks the team's shifts for the planned day", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce({
      team_id: 3,
      day: "2026-04-13",
      options: [
        {
          class_version_id: 11,
          schedule_class_id: 4,
          class_label: "Poste jour",
          shift_start: "06:00",
          shift_end: "14:00",
          break_minutes: 30,
          net_hours: 7.5,
          members: 2,
          members_available: 2,
        },
      ],
      reason: null,
    });
    renderSelect(wo());
    await waitFor(() => expect(svc.listWoShiftOptions).toHaveBeenCalledWith(7, "2026-04-13"));
    expect(screen.queryByText("planning.shiftNoShift")).toBeNull();
  });

  it("explains why nothing is offered and links to the team", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce({
      team_id: 3,
      day: "2026-04-19",
      options: [],
      reason: "no_shift",
    });
    renderSelect(wo(), "2026-04-19");
    expect(await screen.findByText("planning.shiftNoShift")).toBeTruthy();
    expect(screen.getByRole("link", { name: "planning.shiftOpenTeam" }).getAttribute("href")).toBe(
      "/personnel?tab=teams&team=3",
    );
  });

  it("asks for a team, and flags an old shift code to replace", async () => {
    svc.listWoShiftOptions.mockResolvedValueOnce({
      team_id: null,
      day: "2026-04-13",
      options: [],
      reason: "no_team",
    });
    renderSelect(wo({ assigned_team_id: null, shift: "morning" }));
    expect(await screen.findByText("planning.shiftNoTeam")).toBeTruthy();
    expect(screen.getByText(/planning\.shiftLegacy/)).toBeTruthy();
  });
});

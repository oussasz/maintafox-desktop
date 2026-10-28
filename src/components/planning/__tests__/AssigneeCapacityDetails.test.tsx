import { fireEvent, render, screen } from "@testing-library/react";
import type { TFunction } from "i18next";
import { describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import type { AssigneeCapacity } from "@shared/ipc-types";

import { AssigneeCapacityDetails, assigneeOptionSuffix } from "../AssigneeCapacityDetails";
import { OverrideReasonDialog } from "../OverrideReasonDialog";
import { isAssigneeOverridableError } from "../planning-model";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPlanning,
    );
  return typeof value === "string" ? value : undefined;
}

/** The English planning texts, with i18next plurals (`_one` / `_other`) and interpolation. */
function translate(key: string, opts?: Record<string, unknown>): string {
  const count = opts?.["count"];
  const plural =
    typeof count === "number" ? lookup(`${key}_${count === 1 ? "one" : "other"}`) : undefined;
  const text =
    plural ??
    lookup(key) ??
    (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key);
  return text.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate, i18n: { language: "en" } }),
}));

const t = translate as unknown as TFunction<"planning">;

function capacity(overrides: Partial<AssigneeCapacity> = {}): AssigneeCapacity {
  return {
    personnel_id: 7,
    full_name: "Amine Haddad",
    employee_code: "T-07",
    days: [
      {
        day: "2026-05-04",
        shift_start: "08:00",
        shift_end: "16:00",
        available_hours: 8,
        committed_hours: 6,
        remaining_hours: 2,
        reason: "working",
      },
      {
        day: "2026-05-05",
        shift_start: null,
        shift_end: null,
        available_hours: 0,
        committed_hours: 0,
        remaining_hours: 0,
        reason: "rest_day",
      },
    ],
    slot_working_hours: 8,
    issues: [],
    unknown_hours_jobs: 0,
    unreadable_work_orders: [],
    ...overrides,
  };
}

describe("AssigneeCapacityDetails", () => {
  it("shows each day with its shift or why the person does not work, and the hours left", () => {
    render(<AssigneeCapacityDetails capacity={capacity()} assigned />);
    const days = screen.getByRole("list", { name: "The technician's days over the slot" });
    expect(days.textContent).toContain("08:00–16:00");
    expect(days.textContent).toContain("2,0 h left");
    expect(days.textContent).toContain("rest day");
  });

  it("lists the refusals, the jobs with unknown hours and the unreadable work orders", () => {
    render(
      <AssigneeCapacityDetails
        capacity={capacity({
          issues: ["PLANNING_ASSIGNEE_OVERLOADED"],
          unknown_hours_jobs: 2,
          unreadable_work_orders: [{ work_order_id: 3, code: "WO-0003" }],
        })}
        assigned
      />,
    );
    expect(screen.getByText("Not enough hours left for this job on one of the days.")).toBeTruthy();
    expect(screen.getByText("2 of their jobs have unknown hours (not counted).")).toBeTruthy();
    expect(screen.getByText(/WO-0003/)).toBeTruthy();
  });

  it("names how many other crew members the job takes, counted in the crew's load", () => {
    const { rerender } = render(
      <AssigneeCapacityDetails capacity={undefined} crewSize={3} assigned />,
    );
    expect(screen.getByText(/\+ 2 other crew members not named/)).toBeTruthy();
    rerender(<AssigneeCapacityDetails capacity={undefined} crewSize={2} assigned />);
    expect(screen.getByText(/\+ 1 other crew member not named/)).toBeTruthy();
    rerender(<AssigneeCapacityDetails capacity={undefined} crewSize={1} assigned={false} />);
    expect(screen.queryByText(/not named/)).toBeNull();
  });
});

const plainDecimal = (value: number, decimals = 2) => value.toFixed(decimals);

describe("assigneeOptionSuffix", () => {
  it("gives the hours left each day and the shift", () => {
    expect(assigneeOptionSuffix(capacity(), t, plainDecimal)).toBe(
      "2.0 / 0.0 h left · 08:00–16:00",
    );
  });

  it("gives the hours without a shift when none is known", () => {
    const noShift = capacity({
      days: [
        {
          day: "2026-05-04",
          shift_start: null,
          shift_end: null,
          available_hours: 8,
          committed_hours: 0,
          remaining_hours: 8,
          reason: "working",
        },
      ],
    });
    expect(assigneeOptionSuffix(noShift, t, plainDecimal)).toBe("8.0 h left");
  });

  it("says why the person does not work during the slot", () => {
    const off = capacity({ issues: ["PLANNING_ASSIGNEE_NOT_WORKING"], slot_working_hours: 0 });
    expect(assigneeOptionSuffix(off, t, plainDecimal)).toBe("not working (rest day)");
    const outside = capacity({
      slot_working_hours: 0,
      days: [
        {
          day: "2026-05-04",
          shift_start: "08:00",
          shift_end: "16:00",
          available_hours: 8,
          committed_hours: 0,
          remaining_hours: 8,
          reason: "working",
        },
      ],
    });
    expect(assigneeOptionSuffix(outside, t, plainDecimal)).toBe(
      "not working (outside their shift)",
    );
  });
});

describe("isAssigneeOverridableError", () => {
  it("recognises the assignee refusals and a double booking", () => {
    expect(isAssigneeOverridableError(["PLANNING_ASSIGNEE_NOT_IN_CREW"])).toBe(true);
    expect(isAssigneeOverridableError(["PLANNING_DOUBLE_BOOKING"])).toBe(true);
    expect(isAssigneeOverridableError(["PLANNING_NOT_ESTIMATED"])).toBe(false);
  });
});

describe("OverrideReasonDialog", () => {
  it("lists the refusals being overridden and needs a reason to confirm", () => {
    const onConfirm = vi.fn();
    render(
      <OverrideReasonDialog
        open
        onOpenChange={() => undefined}
        issues={["PLANNING_ASSIGNEE_NOT_WORKING"]}
        onConfirm={onConfirm}
      >
        <p>context</p>
      </OverrideReasonDialog>,
    );
    expect(screen.getByText("Override the assignee checks")).toBeTruthy();
    expect(screen.getByText("Does not work during this slot.")).toBeTruthy();
    expect(screen.getByText("context")).toBeTruthy();
    const confirm = screen.getByRole("button", { name: "Confirm override" });
    expect(confirm.hasAttribute("disabled")).toBe(true);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Night callout" } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm override" }));
    expect(onConfirm).toHaveBeenCalledWith("Night callout");
  });

  it("keeps the double-booking wording when no assignee refusal is listed", () => {
    render(
      <OverrideReasonDialog open onOpenChange={() => undefined} onConfirm={() => undefined} />,
    );
    expect(screen.getByText("Override double-booking")).toBeTruthy();
  });
});

/**
 * The PM calendar asks for the dates it shows only (server due-date filter), and again when the
 * user moves to another month or changes the view.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PmCalendarView, calendarRange } from "@/components/pm/PmCalendarView";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

describe("PM calendar range", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 17, 10, 0, 0));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("covers the local days shown, with a day of margin, as UTC text", () => {
    const range = calendarRange(new Date(2026, 8, 28), new Date(2026, 10, 8));
    expect(range.from).toBe(`${new Date(2026, 8, 27).toISOString().slice(0, 19)}Z`);
    expect(range.to).toBe(`${new Date(2026, 10, 9, 23, 59, 59).toISOString().slice(0, 19)}Z`);
  });

  it("asks for the month grid shown, then for the next month", () => {
    const onRange = vi.fn();
    render(
      <PmCalendarView occurrences={[]} onChipClick={vi.fn()} onVisibleRangeChange={onRange} />,
    );
    // October 2026: the grid runs from Monday 28 September to Sunday 8 November.
    expect(onRange).toHaveBeenLastCalledWith(
      calendarRange(new Date(2026, 8, 28), new Date(2026, 10, 8)),
    );
    fireEvent.click(screen.getByRole("button", { name: "calendar.next" }));
    // November 2026: Monday 26 October to Sunday 6 December.
    expect(onRange).toHaveBeenLastCalledWith(
      calendarRange(new Date(2026, 9, 26), new Date(2026, 11, 6)),
    );
    expect(onRange).toHaveBeenCalledTimes(2);
  });
});

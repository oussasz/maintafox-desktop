import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { SmrpIndicatorsSection, pointsTrend } from "@/components/analytics/SmrpIndicatorsSection";
import type { SmrpKpis } from "@/services/analytics-dashboard-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (!opts) return key;
      const vals = Object.entries(opts)
        .filter(([k]) => k !== "defaultValue")
        .map(([k, v]) => `${k}=${String(v)}`);
      return vals.length ? `${key}[${vals.join(",")}]` : key;
    },
    i18n: { language: "en" },
  }),
}));

const ratio = (n: number, d: number, pct: number | null) => ({
  numerator: n,
  denominator: d,
  value_pct: pct,
});

const SMRP: SmrpKpis = {
  from: "2026-09-14T12:00:00Z",
  to: "2026-10-14T12:00:00Z",
  days: 30,
  schedule_compliance: { current: ratio(4, 9, 44.4), previous: ratio(6, 10, 60) },
  work_mix: {
    basis: "hours",
    planned: { current: ratio(5, 12, 41.7), previous: ratio(3, 3, 100) },
    reactive: { current: ratio(7, 12, 58.3), previous: ratio(0, 3, 0) },
    emergency: { current: ratio(2, 12, 16.7), previous: ratio(0, 3, 0) },
    work_orders: 4,
  },
  pm_compliance: { current: ratio(1, 3, 33.3), previous: ratio(0, 0, null) },
  backlog: {
    backlog_hours: 20,
    previous_backlog_hours: 0,
    ready_backlog_hours: 6,
    open_work_orders: 4,
    work_orders_without_estimate: 1,
    weekly_capacity_hours: 64,
    people_counted: 2,
    people_without_schedule: 1,
    weeks: 0.3125,
    previous_weeks: 0,
    ready_weeks: 0.09375,
  },
  cost_to_rav: {
    period_cost: 0,
    annualized_cost: 0,
    replacement_value_total: 0,
    assets_with_value: 0,
    assets_total: 12,
    value_pct: null,
    previous_value_pct: null,
  },
};

function renderSection(smrp: SmrpKpis | null = SMRP) {
  return render(
    <MemoryRouter>
      <SmrpIndicatorsSection smrp={smrp} />
    </MemoryRouter>,
  );
}

describe("pointsTrend", () => {
  it("compares percentages in points and colours by direction", () => {
    expect(pointsTrend(44.4, 60, "up-good")).toMatchObject({ tone: "danger" });
    expect(pointsTrend(58.3, 0, "up-bad")).toMatchObject({ tone: "danger" });
    expect(pointsTrend(41.7, 100, "up-bad")).toMatchObject({ tone: "success" });
    expect(pointsTrend(50.2, 50, "up-good")).toEqual({ points: 0, tone: null });
    expect(pointsTrend(33.3, null, "up-good")).toBeNull();
  });
});

describe("SmrpIndicatorsSection", () => {
  it("shows each indicator with its figures and no trend without a previous value", () => {
    renderSection();
    expect(screen.getByText("smrp.title")).toBeInTheDocument();
    const pm = screen.getByTestId("smrp-pm");
    expect(within(pm).getByText(/smrp\.pm\.detail/)).toBeInTheDocument();
    expect(within(pm).queryByText(/smrp\.trend/)).not.toBeInTheDocument();
    const schedule = screen.getByTestId("smrp-schedule");
    expect(within(schedule).getByText(/smrp\.trendPoints/)).toBeInTheDocument();
    // Backlog states the capacity it used and the people left out.
    const backlog = screen.getByTestId("smrp-backlog");
    expect(within(backlog).getByText(/smrp\.backlog\.capacity/)).toBeInTheDocument();
    expect(within(backlog).getByText(/smrp\.backlog\.withoutSchedule/)).toBeInTheDocument();
  });

  it("says a replacement value is needed instead of inventing a ratio", () => {
    renderSection();
    const rav = screen.getByTestId("smrp-rav");
    expect(within(rav).getByText(/smrp\.rav\.needsValue\[valued=0,total=12\]/)).toBeInTheDocument();
    expect(within(rav).getByRole("button", { name: /smrp\.rav\.openAssets/ })).toBeInTheDocument();
  });

  it("lists wrench time as not measured", () => {
    renderSection();
    expect(screen.getByText("smrp.wrenchTime")).toBeInTheDocument();
  });

  it("opens the definition (formula, period, unit) of an indicator", () => {
    renderSection();
    fireEvent.click(
      screen.getAllByRole("button", { name: /smrp\.definitionAria/ })[0] as HTMLElement,
    );
    expect(screen.getByText("smrp.schedule.formula")).toBeInTheDocument();
    expect(screen.getByText("smrp.unitPercent")).toBeInTheDocument();
  });

  it("renders nothing without data", () => {
    const { container } = renderSection(null);
    expect(container).toBeEmptyDOMElement();
  });
});

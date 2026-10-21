import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { AnalyticsDiStatusChart } from "@/components/analytics/AnalyticsDiStatusChart";
import { periodTrend } from "@/components/analytics/analytics-kpi-trend";
import type { AnalyticsKpiValue } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

function kpi(value: number, previous: number, extra: Partial<AnalyticsKpiValue> = {}) {
  return {
    key: "k",
    value,
    previous_value: previous,
    available: true,
    quality_badge: null,
    catalog_id: null,
    ...extra,
  };
}

describe("periodTrend", () => {
  it("returns no trend when there is no baseline (instead of 'no change')", () => {
    expect(periodTrend(kpi(5, 0), "up-good")).toBeNull();
    expect(
      periodTrend(kpi(5, 4, { quality_badge: "insufficient_baseline" }), "up-good"),
    ).toBeNull();
    expect(periodTrend(kpi(5, 4, { available: false }), "up-good")).toBeNull();
    expect(periodTrend(undefined, "up-good")).toBeNull();
  });

  it("is stable for an unchanged or tiny change", () => {
    expect(periodTrend(kpi(0, 0), "up-bad")).toEqual({ percent: 0, sense: "neutral" });
    expect(periodTrend(kpi(100.2, 100), "up-bad")).toEqual({ percent: 0, sense: "neutral" });
  });

  it("colours the change by whether a rise is good or bad", () => {
    expect(periodTrend(kpi(120, 100), "up-good")).toMatchObject({ sense: "up-good" });
    expect(periodTrend(kpi(120, 100), "up-bad")).toMatchObject({ sense: "up-bad" });
    expect(periodTrend(kpi(80, 100), "up-bad")).toMatchObject({ sense: "up-good" });
    expect(periodTrend(kpi(80, 100), "up-good")?.percent).toBeCloseTo(-20);
  });
});

describe("AnalyticsDiStatusChart", () => {
  it("labels statuses in words, never the raw workflow code", () => {
    render(
      <MemoryRouter>
        <AnalyticsDiStatusChart
          segments={[
            { status: "returned_for_clarification", count: 3 },
            { status: "closed", count: 1 },
          ]}
        />
      </MemoryRouter>,
    );
    expect(
      screen.getByText("hub.dashboards.diStatus.returned_for_clarification"),
    ).toBeInTheDocument();
    expect(screen.queryByText("returned_for_clarification")).not.toBeInTheDocument();
  });

  it("shows an empty message instead of disappearing", () => {
    render(
      <MemoryRouter>
        <AnalyticsDiStatusChart segments={[]} />
      </MemoryRouter>,
    );
    expect(screen.getByText("hub.dashboards.diStatus.empty")).toBeInTheDocument();
  });
});

import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { OverviewMetricCards } from "@/components/dashboard/OverviewMetricCards";
import type { OverviewKpiCell } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "change" in opts ? `${key}:${String(opts["change"])}` : key,
    i18n: { language: "en" },
  }),
}));

function cell(over: Partial<OverviewKpiCell>): OverviewKpiCell {
  return {
    semantic_key: "PM_COMPLIANCE",
    catalog_id: "KPI-036",
    value: 80,
    previous_value: 72,
    available: true,
    value_kind: "percent",
    trend_sense: "up-good",
    ...over,
  };
}

function renderCards(c: OverviewKpiCell) {
  return render(
    <MemoryRouter>
      <OverviewMetricCards
        overviewKpis={{ horizon_days: 30, cells: [c] }}
        layout={[{ id: c.semantic_key, order: 0, visible: true }]}
      />
    </MemoryRouter>,
  );
}

describe("OverviewMetricCards trend", () => {
  it("shows a percentage KPI in points, not as a relative percent", () => {
    renderCards(cell({}));
    expect(screen.getByText("kpi.points:+8")).toBeInTheDocument();
  });

  it("reads stable below the KPI's own threshold", () => {
    renderCards(cell({ value: 80.2, previous_value: 80 }));
    expect(screen.getByText("kpi.stable")).toBeInTheDocument();
  });

  it("keeps the relative percent for a count KPI", () => {
    renderCards(
      cell({
        semantic_key: "DI_OPEN",
        value_kind: "count",
        value: 12,
        previous_value: 10,
        trend_sense: "up-bad",
      }),
    );
    expect(screen.getByText(/\+20\s%/)).toBeInTheDocument();
  });
});

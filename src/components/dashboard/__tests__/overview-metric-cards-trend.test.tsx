import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { OverviewMetricCards } from "@/components/dashboard/OverviewMetricCards";
import type { OverviewKpiCell } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "change" in opts
        ? `${key}:${String(opts["change"])}`
        : opts && "basis" in opts
          ? `${key}[${String(opts["basis"])}]`
          : key,
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

  it("words the KPI's own band on the trend badge, from the registry", () => {
    renderCards(cell({}));
    expect(screen.getByText("kpi.points:+8")).toHaveAttribute("title", "smrp.stableBand");
  });

  it("uses the registry's sense, not the one the backend sent (PM due today is neutral)", () => {
    renderCards(
      cell({
        semantic_key: "PM_DUE_TODAY",
        value_kind: "count",
        value: 6,
        previous_value: 2,
        trend_sense: "up-bad",
      }),
    );
    expect(screen.getByText("+4").className).not.toContain("status-danger");
  });

  it("shows a small count change in units, never as a percent (L7-08)", () => {
    renderCards(
      cell({
        semantic_key: "DI_OPEN",
        value_kind: "count",
        value: 4,
        previous_value: 1,
        trend_sense: "up-bad",
      }),
    );
    expect(screen.getByText("+3")).toBeInTheDocument();
    expect(screen.queryByText(/300/)).toBeNull();
  });

  /** Live figures of a fleet cell: 12 months, calendar hours, 2 outside service, 1 not restored. */
  const fleet = (over: Partial<NonNullable<OverviewKpiCell["fleet"]>["kpis"]> = {}) => ({
    window_months: 12,
    kpis: {
      window_too_short: false,
      operating_basis_count: 0,
      calendar_basis_count: 4,
      mtbf_excluded_missing_date: 0,
      mtbf_excluded_outside_service: 2,
      restore_excluded_missing_failed_at: 0,
      restore_excluded_missing_restored_at: 1,
      restore_excluded_inconsistent: 0,
      ...over,
    },
  });
  const fleetCell = (key: string, over: Partial<OverviewKpiCell> = {}) =>
    cell({
      semantic_key: key,
      value_kind: key === "AVAILABILITY" || key === "DATA_QUALITY" ? "percent" : "hours",
      value: 410,
      previous_value: 400,
      fleet: fleet(),
      ...over,
    });

  it("words each fleet card like the RAMS home: basis on MTBF and Aₒ only, exclusions where shown", () => {
    // The mock shows the hours basis a hint receives as "[basis]".
    const cases: [string, string][] = [
      ["MTBF", "rams.kpi.mtbfHint[rams.fleet.basisCalendar] · rams.kpi.excluded.outsideService"],
      ["AVAILABILITY", "rams.kpi.availabilityHint[rams.fleet.basisCalendar]"],
      ["MTTR", "rams.kpi.mttrHint · rams.kpi.excluded.restoredAt"],
      ["DATA_QUALITY", "rams.fleet.periodMonths"],
    ];
    for (const [key, context] of cases) {
      const view = renderCards(fleetCell(key));
      expect(screen.getByText(context)).toBeInTheDocument();
      expect(screen.queryByText("overviewKpi.contextFleet")).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it("says the period is too short for the MTBF under 90 days", () => {
    renderCards(fleetCell("MTBF", { available: false, fleet: fleet({ window_too_short: true }) }));
    expect(screen.getByText("rams.fleet.periodMonths · rams.kpi.tooShort")).toBeInTheDocument();
  });
});

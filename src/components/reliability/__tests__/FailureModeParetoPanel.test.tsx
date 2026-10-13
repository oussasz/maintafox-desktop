import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  FailureModeParetoPanel,
  dayBound,
  paretoChartData,
  vitalFewCount,
} from "../FailureModeParetoPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

vi.mock("recharts", () => {
  const Box = ({ children }: { children?: unknown }) => <div>{children as never}</div>;
  return {
    Bar: Box,
    CartesianGrid: Box,
    ComposedChart: Box,
    Line: Box,
    ReferenceLine: Box,
    ResponsiveContainer: Box,
    Tooltip: Box,
    XAxis: Box,
    YAxis: Box,
  };
});

const svc = vi.hoisted(() => ({
  getFailureModePareto: vi.fn(),
  exportReliabilityReport: vi.fn(),
}));
vi.mock("@/services/reliability-service", () => svc);
const reports = vi.hoisted(() => ({ downloadExportedDocument: vi.fn() }));
vi.mock("@/services/reports-service", () => reports);
const perms = vi.hoisted(() => ({ allowed: true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => perms.allowed, isLoading: false }),
}));

function row(label: string | null, value: number, cumulative: number, id: number | null = 1) {
  return {
    failure_mode_id: id,
    mode_code: label ? label.toUpperCase() : null,
    mode_label: label,
    failure_count: value,
    total_cost: value * 10,
    downtime_hours: value * 2,
    value,
    share: 0,
    cumulative_share: cumulative,
  };
}

describe("pareto helpers", () => {
  it("labels unclassified failures and rounds the cumulative percentage", () => {
    const pts = paretoChartData([row("Bearing", 3, 0.6), row(null, 2, 1, null)], "none");
    expect(pts).toEqual([
      { label: "Bearing", value: 3, cumulativePct: 60 },
      { label: "none", value: 2, cumulativePct: 100 },
    ]);
  });

  it("counts the modes reaching 80 %", () => {
    expect(vitalFewCount([row("a", 5, 0.5), row("b", 3, 0.8), row("c", 2, 1)])).toBe(2);
    expect(vitalFewCount([row("a", 1, 0.5)])).toBe(1);
    expect(vitalFewCount([])).toBe(0);
  });

  it("turns date inputs into whole-day UTC bounds", () => {
    expect(dayBound("", false)).toBeNull();
    expect(dayBound("2026-03-01", false)).toBe("2026-03-01T00:00:00Z");
    expect(dayBound("2026-03-31", true)).toBe("2026-03-31T23:59:59Z");
  });
});

describe("FailureModeParetoPanel", () => {
  it("loads the Pareto for the equipment and reloads when the metric changes", async () => {
    svc.getFailureModePareto.mockResolvedValue({
      metric: "count",
      total: 5,
      rows: [row("Bearing", 3, 0.6), row("Belt", 2, 1, 2)],
    });
    render(<FailureModeParetoPanel equipmentId={7} />);
    expect(await screen.findByText("Bearing")).toBeTruthy();
    expect(svc.getFailureModePareto).toHaveBeenCalledWith({
      metric: "count",
      equipment_id: 7,
      period_start: null,
      period_end: null,
    });
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "cost" } });
    await waitFor(() =>
      expect(svc.getFailureModePareto).toHaveBeenLastCalledWith(
        expect.objectContaining({ metric: "cost" }),
      ),
    );
  });

  it("shows load errors instead of an empty chart", async () => {
    svc.getFailureModePareto.mockRejectedValue(new Error("db locked"));
    render(<FailureModeParetoPanel equipmentId={7} />);
    expect((await screen.findByRole("alert")).textContent).toBe("pareto.loadFailed");
  });

  it("exports the report and surfaces export failures", async () => {
    perms.allowed = true;
    svc.getFailureModePareto.mockResolvedValue({ metric: "count", total: 0, rows: [] });
    const doc = { file_name: "r.pdf", mime_type: "application/pdf", bytes: [37] };
    svc.exportReliabilityReport
      .mockResolvedValueOnce(doc)
      .mockRejectedValueOnce(new Error("no font"));
    render(<FailureModeParetoPanel equipmentId={7} />);
    expect(await screen.findByText("pareto.empty")).toBeTruthy();
    fireEvent.click(screen.getByText("pareto.export.pdf"));
    await waitFor(() => expect(reports.downloadExportedDocument).toHaveBeenCalledWith(doc));
    fireEvent.click(screen.getByText("pareto.export.xlsx"));
    expect((await screen.findByRole("alert")).textContent).toBe("pareto.exportFailed");
    expect(svc.exportReliabilityReport).toHaveBeenLastCalledWith(7, "xlsx");
  });

  it("hides export without the export permission", async () => {
    perms.allowed = false;
    svc.getFailureModePareto.mockResolvedValue({ metric: "count", total: 0, rows: [] });
    render(<FailureModeParetoPanel equipmentId={7} />);
    expect(await screen.findByText("pareto.empty")).toBeTruthy();
    expect(screen.queryByText("pareto.export.pdf")).toBeNull();
  });
});

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { WoBacklogDay, WoBacklogHeatmap as Heatmap } from "@shared/ipc-types";

import { WoBacklogHeatmap, loadLevel } from "../WoBacklogHeatmap";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}:${JSON.stringify(opts)}` : key,
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({ getWoBacklogHeatmap: vi.fn() }));
vi.mock("@/services/wo-service", () => svc);

function day(
  date: string,
  hours: number,
  capacity: number | null,
  overloaded = false,
): WoBacklogDay {
  return {
    date,
    planned_hours: hours,
    wo_count: hours > 0 ? 1 : 0,
    unestimated_count: 0,
    capacity_hours: capacity,
    overloaded,
  };
}

describe("loadLevel", () => {
  it("uses capacity when known and marks overload", () => {
    expect(loadLevel(day("2026-10-12", 0, 8), 10)).toBe(0);
    expect(loadLevel(day("2026-10-12", 2, 8), 10)).toBe(1);
    expect(loadLevel(day("2026-10-12", 5, 8), 10)).toBe(2);
    expect(loadLevel(day("2026-10-12", 7, 8), 10)).toBe(3);
    expect(loadLevel(day("2026-10-12", 9, 8, true), 10)).toBe(4);
  });
  it("falls back to the busiest day without capacity", () => {
    expect(loadLevel(day("2026-10-12", 10, null), 10)).toBe(3);
    expect(loadLevel(day("2026-10-12", 1, null), 10)).toBe(1);
  });
});

describe("WoBacklogHeatmap", () => {
  it("renders the grid, the overload ring and the blocked categories", async () => {
    const days = Array.from({ length: 28 }, (_, i) =>
      day(`2026-10-${String(12 + (i % 15)).padStart(2, "0")}`, i === 1 ? 10 : 0, 8, i === 1),
    );
    const data: Heatmap = {
      start_date: "2026-10-12",
      days,
      capacity_configured: true,
      unscheduled: { wo_count: 2, estimated_hours: 3, unestimated_count: 1 },
      overdue: { wo_count: 1, estimated_hours: 1, unestimated_count: 0 },
      blocked: [{ category: "parts", wo_count: 1, waiting_hours: 4 }],
    };
    svc.getWoBacklogHeatmap.mockResolvedValue(data);
    render(<WoBacklogHeatmap />);
    const cells = await screen.findAllByRole("gridcell");
    expect(cells).toHaveLength(28);
    expect(cells[1]).toHaveAttribute("data-level", "4");
    expect(screen.getByText(/dashboard.backlogHeatmap.category.parts/)).toBeInTheDocument();
    expect(screen.queryByText("dashboard.backlogHeatmap.noCapacity")).not.toBeInTheDocument();
    expect(svc.getWoBacklogHeatmap).toHaveBeenCalledWith({ weeks: 4 });
  });

  it("says when no capacity is configured", async () => {
    svc.getWoBacklogHeatmap.mockResolvedValue({
      start_date: "2026-10-12",
      days: [day("2026-10-12", 3, null)],
      capacity_configured: false,
      unscheduled: { wo_count: 0, estimated_hours: 0, unestimated_count: 0 },
      overdue: { wo_count: 0, estimated_hours: 0, unestimated_count: 0 },
      blocked: [],
    });
    render(<WoBacklogHeatmap />);
    expect(await screen.findByText("dashboard.backlogHeatmap.noCapacity")).toBeInTheDocument();
  });
});

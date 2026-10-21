import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AvailabilityCalendar } from "@/components/personnel/AvailabilityCalendar";
import { toLocalIsoDate } from "@/components/personnel/availability-model";
import type { AvailabilityCalendarEntry, PersonnelAvailabilityBlock } from "@shared/ipc-types";

// A stable `t` like the real one (a new function per render would re-run every memo).
const i18nMock = vi.hoisted(() => {
  const t = (key: string, opts?: unknown) =>
    typeof opts === "object" && opts !== null && "count" in opts
      ? `${key}:${String((opts as { count: number }).count)}`
      : key;
  return { t, i18n: { language: "en" } };
});
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
// The calendar offers the absence-type filter to holders of the sensitive-data right.
vi.mock("@/hooks/use-permissions", () => ({ usePermissions: () => ({ can: () => true }) }));

/** Lightweight stand-in for the canvas: rows (titles) and bars (titles) as text. */
vi.mock("@/components/timeline/ResourceTimeline", () => ({
  ResourceTimeline: ({
    groups,
    items,
  }: {
    groups: { id: number; title: ReactNode }[];
    items: { id: number; group: number; title: string }[];
  }) => (
    <div>
      {groups.map((g) => (
        <div key={g.id} data-testid={`row-${g.id}`}>
          {g.title}
          {items
            .filter((i) => i.group === g.id)
            .map((i) => (
              <span key={i.id} data-testid={`bar-${g.id}`}>
                {i.title}
              </span>
            ))}
        </div>
      ))}
    </div>
  ),
}));

const svc = vi.hoisted(() => ({
  listAvailabilityCalendar: vi.fn(),
  listPersonnelAvailabilityBlocks: vi.fn(),
  listTeamCapacitySummary: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);
vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn().mockResolvedValue({ nodes: [] }),
}));
vi.mock("@/services/qualification-service", () => ({
  listCertificationExpiryDrilldown: vi.fn().mockResolvedValue([]),
}));

const today = toLocalIsoDate(new Date());

function entry(personnel_id: number, name: string): AvailabilityCalendarEntry {
  return {
    personnel_id,
    employee_code: `E${personnel_id}`,
    full_name: name,
    entity_id: null,
    entity_name: null,
    team_id: null,
    team_name: null,
    work_date: today,
    shift_start: "08:00",
    shift_end: "16:00",
    scheduled_minutes: 480,
    blocked_minutes: 0,
    available_minutes: 480,
    has_critical_block: false,
    block_types: [],
  };
}

function block(id: number, personnel_id: number, start: string): PersonnelAvailabilityBlock {
  return {
    id,
    personnel_id,
    block_type: "leave",
    start_at: start,
    end_at: start,
    reason_note: null,
    is_critical: false,
    created_by_id: null,
    created_by_label: null,
    created_at: start,
    reason_masked: false,
    type_masked: false,
    critical_for_planning: false,
  };
}

describe("AvailabilityCalendar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listAvailabilityCalendar.mockResolvedValue([
      entry(1, "Ada Lovelace"),
      entry(2, "Bob Ross"),
    ]);
    svc.listTeamCapacitySummary.mockResolvedValue([]);
  });

  it("never shows a person as available when their absences could not be read", async () => {
    svc.listPersonnelAvailabilityBlocks.mockImplementation(async (id: number) => {
      if (id === 2) throw { code: "DATABASE_ERROR", message: "locked" };
      return [];
    });
    render(<AvailabilityCalendar />);
    expect(await screen.findByTestId("bar-2")).toHaveTextContent("availability.unknownBar");
    expect(screen.getByTestId("row-2")).toHaveTextContent("availability.badges.unknown");
    expect(screen.getByTestId("row-1")).not.toHaveTextContent("availability.badges.unknown");
    expect(screen.queryByTestId("bar-1")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent("availability.unknownTitle:1");
    expect(svc.listPersonnelAvailabilityBlocks).toHaveBeenCalledWith(1, 200);

    // Retry reads again; once it succeeds the person is no longer unknown.
    svc.listPersonnelAvailabilityBlocks.mockResolvedValue([]);
    fireEvent.click(screen.getByRole("button", { name: /retry/i }));
    await waitFor(() => expect(screen.queryByTestId("bar-2")).toBeNull());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("flags a full page that may hide older absences of the horizon", async () => {
    const future = new Date(Date.now() + 2 * 86_400_000).toISOString();
    svc.listPersonnelAvailabilityBlocks.mockImplementation(async (id: number) =>
      id === 1 ? Array.from({ length: 200 }, (_, i) => block(i + 1, 1, future)) : [],
    );
    render(<AvailabilityCalendar />);
    await waitFor(() =>
      expect(screen.getByTestId("row-1")).toHaveTextContent("availability.badges.unknown"),
    );
    expect(screen.getByTestId("row-2")).not.toHaveTextContent("availability.badges.unknown");
  });
});

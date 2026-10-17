import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPersonnel from "@/i18n/locale-data/en/personnel.json";
import type * as Workforce from "@/services/workforce-service";
import type { TeamDayCapacity, TeamScheduleAssignment } from "@/services/workforce-service";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPersonnel,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const count = typeof opts?.["count"] === "number" ? opts["count"] : null;
      const text =
        (count != null ? lookup(`${key}_${count === 1 ? "one" : "other"}`) : undefined) ??
        lookup(key) ??
        key;
      return text.replace(/\{\{(\w+)\}\}/g, (_, k: string) => String(opts?.[k] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const canMock = vi.fn<(perm: string) => boolean>();
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: canMock }),
}));

const capacityMock = vi.fn<() => Promise<TeamDayCapacity[]>>();
const linksMock = vi.fn<() => Promise<TeamScheduleAssignment[]>>();
const confirmMock = vi.fn<() => Promise<TeamScheduleAssignment[]>>();
vi.mock("@/services/workforce-service", async (orig) => ({
  ...(await orig<typeof Workforce>()),
  getTeamCapacity: () => capacityMock(),
  listTeamSchedule: () => linksMock(),
  listScheduleClassVersions: () => Promise.resolve([]),
  confirmTeamScheduleReview: () => confirmMock(),
}));

import { TeamTimetablePanel } from "../TeamTimetablePanel";

function day(
  members: TeamDayCapacity["members"],
  basis: TeamDayCapacity["basis"],
): TeamDayCapacity {
  return {
    team_id: 2,
    day: "2026-10-18",
    schedule_class_id: null,
    schedule_class_label: null,
    available_hours: 6.8,
    attendance_hours: 8,
    scheduled_hours: 8,
    absent_hours: 0,
    people: members.length,
    people_scheduled: 1,
    people_absent: 0,
    basis,
    rule_overtime_hours: null,
    members,
  };
}

const withClass = {
  personnel_id: 7,
  full_name: "Karim",
  allocation_percent: 100,
  schedule_class_label: "Journée",
  schedule_source: "personal" as const,
  available_hours: 6.8,
  reason: "working" as const,
};
const without = {
  personnel_id: 8,
  full_name: "Lina",
  allocation_percent: 100,
  schedule_class_label: null,
  schedule_source: null,
  available_hours: 0,
  reason: "no_schedule" as const,
};

describe("TeamTimetablePanel", () => {
  beforeEach(() => {
    canMock.mockReturnValue(true);
    linksMock.mockResolvedValue([]);
  });

  it("lists members without a Classe horaire with a link to their sheet, never assuming hours", async () => {
    capacityMock.mockResolvedValue([day([withClass, without], "roster")]);
    const open = vi.fn();
    render(<TeamTimetablePanel teamId={2} onOpenPersonnel={open} />);
    expect(await screen.findByText("1 member without a Classe horaire")).toBeInTheDocument();
    expect(screen.getByText(/They count 0 h/)).toBeInTheDocument();
    expect(screen.getByText(/each member counts their own Classe horaire/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Set for Lina" }));
    expect(open).toHaveBeenCalledWith(8);
    expect(screen.queryByRole("button", { name: "Set for Karim" })).not.toBeInTheDocument();
  });

  it("explains a team with no hours at all, and confirms a default to review", async () => {
    capacityMock.mockResolvedValue([day([without], "no_schedule")]);
    const migrated: TeamScheduleAssignment = {
      id: 1,
      entity_sync_id: "l-1",
      team_id: 2,
      schedule_reference_value_id: 4,
      schedule_code: "CAP_RULE_EQ_A",
      schedule_label: "Mécanique — 7.5 h/day (former capacity rule)",
      effective_from: "2000-01-01",
      review_status: "to_review",
      row_version: 1,
    };
    linksMock.mockResolvedValue([migrated]);
    confirmMock.mockResolvedValue([{ ...migrated, review_status: null }]);
    render(<TeamTimetablePanel teamId={2} />);
    expect(await screen.findByText(/No member has a Classe horaire/)).toBeInTheDocument();
    expect(screen.getByText(/former capacity rule — please review/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Confirm as is" }));
    await waitFor(() => expect(confirmMock).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByText(/former capacity rule — please review/)).not.toBeInTheDocument(),
    );
  });
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/ipc-invoke";
import { createScheduleCommitment } from "@/services/planning-service";
import type { ScheduleCandidate } from "@shared/ipc-types";

import { CommitDialog } from "../CommitDialog";

vi.mock("@/lib/ipc-invoke", () => ({ invoke: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({
    formatCurrency: (n: number) => String(n),
    formatDecimal: (n: number) => String(n),
  }),
}));
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/components/personnel/use-personnel-lists", () => ({
  useAbsenceTypeLookup: () => () => undefined,
}));
// The day-by-day capacity gate is not under test here: the slot always has room.
vi.mock("@/components/planning/planning-model", async () => ({
  ...(await vi.importActual<Record<string, unknown>>("@/components/planning/planning-model")),
  checkSlotCapacity: () => ({ days: [], overloaded: null, noSchedule: false }),
}));
vi.mock("@/components/timeline/ResourceTimeline", () => ({ ResourceTimeline: () => null }));
vi.mock("@/components/planning/FrozenWeekBreakInDialog", () => ({
  FrozenWeekBreakInDialog: ({ open, weekStart }: { open: boolean; weekStart: string | null }) =>
    open ? <div data-testid="frozen-dialog">{weekStart}</div> : null,
}));
vi.mock("@/components/planning/OverrideReasonDialog", () => ({
  OverrideReasonDialog: ({ open, issues }: { open: boolean; issues: string[] }) =>
    open ? <div data-testid="override-dialog">{issues.join(",")}</div> : null,
}));

const mockInvoke = vi.mocked(invoke);

const candidate = {
  id: 1,
  row_version: 3,
  source_type: "work_order",
  source_id: 44,
  source_code: "OT-0044",
  source_title: "Pump overhaul",
  readiness_status: "ready",
  readiness_score: 80,
  priority_label: null,
  assigned_personnel_id: null,
  assigned_team_id: 7,
  estimated_duration_hours: 2,
  planned_crew_size: 1,
  window_start: "2026-11-03T08:00:00Z",
  window_end: "2026-11-03T16:00:00Z",
} as unknown as ScheduleCandidate;

function renderDialog() {
  return render(
    <MemoryRouter>
      <CommitDialog
        open
        onOpenChange={() => {}}
        candidate={candidate}
        boardScope={{ kind: "team", teamId: 7 } as never}
        teams={[{ id: 7, label: "Crew" }]}
        teamId={7}
        gantt={null}
        commitments={[]}
        lockedWindows={[]}
        horizonStart={0}
        horizonEnd={4_000_000_000_000}
        visibleStart={0}
        visibleEnd={4_000_000_000_000}
        saving={false}
        onSubmit={async (input) => {
          await createScheduleCommitment(input);
        }}
      />
    </MemoryRouter>,
  );
}

/** The real backend shape of a refused commit (AppError serialisation: VALIDATION_FAILED + issues). */
function refuse(code: string, params: Record<string, string> = {}) {
  mockInvoke.mockImplementation((cmd: string) =>
    cmd === "create_schedule_commitment"
      ? Promise.reject({
          code: "VALIDATION_FAILED",
          message: "refused",
          details: [{ code, message: "refused", params }],
        })
      : Promise.resolve([]),
  );
}

describe("CommitDialog refusals reach the dialogs through the service wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("opens the frozen-week break-in dialog on PLANNING_WEEK_FROZEN", async () => {
    refuse("PLANNING_WEEK_FROZEN", { week_start: "2026-11-02" });
    renderDialog();
    fireEvent.click(await screen.findByRole("button", { name: "actions.commit" }));
    expect((await screen.findByTestId("frozen-dialog")).textContent).toBe("2026-11-02");
  });

  it("offers the override step on a double-booking refusal", async () => {
    refuse("PLANNING_DOUBLE_BOOKING");
    renderDialog();
    fireEvent.click(await screen.findByRole("button", { name: "actions.commit" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "commitment.overrideDoubleBooking" }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("override-dialog").textContent).toBe("PLANNING_DOUBLE_BOOKING"),
    );
  });
});

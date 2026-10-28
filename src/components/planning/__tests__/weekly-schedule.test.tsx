import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import type { ReferenceValue, ScheduleCommitment, WeeklyScheduleView } from "@shared/ipc-types";

import { CrewsWithoutSiteBanner } from "../CrewsWithoutSiteBanner";
import { FrozenWeekBreakInDialog } from "../FrozenWeekBreakInDialog";
import { WeeklySchedulePanel } from "../WeeklySchedulePanel";
import { addDays, commitmentsOnDay, daysBetween } from "../weekly-schedule-model";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPlanning,
    );
  return typeof value === "string" ? value : undefined;
}

/** English planning texts; a planning key without text fails the test (shared UI keys pass through). */
function translate(key: string, opts?: Record<string, unknown>): string {
  const text = lookup(key);
  if (text == null) {
    if ((key.split(".")[0] ?? "") in enPlanning)
      throw new Error(`missing planning i18n key: ${key}`);
    return key;
  }
  return text.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate, i18n: { language: "en" } }),
}));

// Radix Select does not run in jsdom: a native <select> with the same props.
vi.mock("@/components/ui/select", () => ({
  Select: ({
    value,
    onValueChange,
    children,
  }: {
    value: string;
    onValueChange: (v: string) => void;
    children: ReactNode;
  }) => (
    <select value={value} onChange={(e) => onValueChange(e.target.value)}>
      <option value="" />
      {children}
    </select>
  ),
  SelectTrigger: () => null,
  SelectValue: () => null,
  SelectContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  SelectItem: ({ value, children }: { value: string; children: ReactNode }) => (
    <option value={value}>{children}</option>
  ),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

const reasons: ReferenceValue[] = [
  reason(1, "emergency", "Urgence", "planning_break_in_urgent"),
  reason(2, "other", "Autre", "planning_break_in"),
];

function reason(id: number, code: string, label: string, tag: string): ReferenceValue {
  return {
    id,
    set_id: 1,
    parent_id: null,
    code,
    label,
    description: null,
    sort_order: id,
    color_hex: null,
    icon_name: null,
    semantic_tag: tag,
    external_code: null,
    is_active: true,
    metadata_json: JSON.stringify({ label_en: code === "emergency" ? "Emergency" : "Other" }),
  };
}

vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn(() => Promise.resolve(reasons)),
}));

const planning = vi.hoisted(() => ({
  getWeeklySchedule: vi.fn(),
  listCrewsWithoutPlanningSite: vi.fn(),
  approveWeeklySchedule: vi.fn(),
  freezeWeeklySchedule: vi.fn(),
  publishWeeklySchedule: vi.fn(),
  requestWeeklyUnfreeze: vi.fn(),
  decideWeeklyUnfreeze: vi.fn(),
  getPlanningKpis: vi.fn(),
  listBreakInApprovers: vi.fn(),
}));
vi.mock("@/services/planning-service", () => planning);

const kpisFixture = {
  weeks: [
    {
      weekly_schedule_id: 4,
      site_id: 10,
      site_name: "Usine Nord",
      week_start: "2026-10-11",
      week_end: "2026-10-17",
      status: "frozen",
      awaiting_approval: true,
      reopened: true,
      baseline_no: 1,
      baseline_jobs: 2,
      baseline_hours: 5,
      baseline_released_jobs: 1,
      done_hours: 2,
      compliance_pct: 40,
      break_in_jobs: 1,
      break_in_hours: 1,
      break_in_pct: 20,
      displaced_jobs: 1,
      available_hours: 40,
      overtime_hours: 8,
      loading_pct: 12.5,
    },
  ],
  totals: {
    weeks: 1,
    weeks_judged: 1,
    weeks_awaiting_approval: 1,
    weeks_reopened: 1,
    weeks_not_frozen: 2,
    baseline_hours: 5,
    baseline_released_jobs: 1,
    judged_baseline_hours: 5,
    done_hours: 2,
    compliance_pct: 40,
    break_in_hours: 1,
    break_in_pct: 20,
    displaced_jobs: 1,
    available_hours: 40,
    overtime_hours: 8,
    loading_pct: 12.5,
  },
};

vi.mock("@/services/org-node-service", () => ({
  listOrgTree: vi.fn(() =>
    Promise.resolve([
      {
        node: { id: 10, name: "Usine Nord", status: "active" },
        node_type_code: "SITE",
        node_type_label: "Site",
        can_host_assets: true,
        can_own_work: true,
        can_carry_cost_center: false,
        can_aggregate_kpis: true,
        can_receive_permits: true,
        child_count: 0,
      },
    ]),
  ),
}));

function commitment(overrides: Partial<ScheduleCommitment>): ScheduleCommitment {
  return {
    id: 1,
    schedule_candidate_id: 1,
    source_type: "work_order",
    source_id: 5,
    assigned_team_id: 3,
    assigned_personnel_id: null,
    committed_start: "2026-10-19T08:00:00",
    committed_end: "2026-10-19T12:00:00",
    frozen_at: null,
    has_blocking_conflict: 0,
    status: "committed",
    status_reason: null,
    status_changed_at: null,
    row_version: 1,
    source_code: "OT-0005",
    source_title: "Pompe P-12",
    assigned_team_label: "MEC",
    assigned_personnel_name: null,
    ...overrides,
  } as ScheduleCommitment;
}

function weekView(overrides: Partial<WeeklyScheduleView> = {}): WeeklyScheduleView {
  return {
    week: {
      entity_id: 10,
      week_start: "2026-10-18",
      week_end: "2026-10-24",
      working_days: ["2026-10-18", "2026-10-19", "2026-10-20", "2026-10-21", "2026-10-22"],
      last_working_day: "2026-10-22",
    },
    site_name: "Usine Nord",
    schedule: {
      id: 4,
      entity_id: 10,
      week_start: "2026-10-18",
      week_end: "2026-10-24",
      status: "frozen",
      approved_by_id: null,
      approved_at: null,
      frozen_at: "2026-10-17T00:00:00Z",
      frozen_by_id: null,
      frozen_without_approval: 1,
      baseline_no: 1,
      published_by_id: null,
      published_at: null,
      unfreeze_requested_by_id: null,
      unfreeze_requested_at: null,
      unfreeze_reason: null,
      row_version: 7,
      created_at: "2026-10-10T00:00:00Z",
      updated_at: "2026-10-17T00:00:00Z",
    },
    awaiting_approval: true,
    freeze_due_at: "2026-10-17T00:00:00Z",
    crews: [{ team_id: 3, team_code: "MEC", team_name: "Mécanique" }],
    commitments: [commitment({})],
    events: [
      {
        id: 1,
        action: "auto_frozen",
        actor_id: null,
        actor_name: null,
        approver_id: null,
        approver_name: null,
        reason: null,
        baseline_no: 1,
        created_at: "2026-10-17T00:00:00Z",
      },
    ],
    ...overrides,
  };
}

describe("weekly schedule helpers", () => {
  it("adds days across a month end", () => {
    expect(addDays("2026-10-30", 3)).toBe("2026-11-02");
    expect(addDays("2026-11-02", -7)).toBe("2026-10-26");
  });

  it("lists the days of a week, including one stretched by a weekend change", () => {
    expect(daysBetween("2026-10-18", "2026-10-24")).toHaveLength(7);
    expect(daysBetween("2026-06-07", "2026-06-14")).toEqual([
      "2026-06-07",
      "2026-06-08",
      "2026-06-09",
      "2026-06-10",
      "2026-06-11",
      "2026-06-12",
      "2026-06-13",
      "2026-06-14",
    ]);
  });

  it("puts a slot on every day it touches, and an end at midnight on the day before", () => {
    const rows = [
      commitment({
        id: 1,
        committed_start: "2026-10-19T22:00:00",
        committed_end: "2026-10-21T00:00:00",
      }),
      commitment({ id: 2, assigned_team_id: 9 }),
    ];
    expect(commitmentsOnDay(rows, 3, "2026-10-19").map((c) => c.id)).toEqual([1]);
    expect(commitmentsOnDay(rows, 3, "2026-10-20").map((c) => c.id)).toEqual([1]);
    expect(commitmentsOnDay(rows, 3, "2026-10-21")).toEqual([]);
  });
});

describe("FrozenWeekBreakInDialog", () => {
  beforeEach(() => {
    planning.listBreakInApprovers.mockResolvedValue([
      { user_id: 58, display_name: "K. Benali", personnel_id: 7 },
    ]);
  });

  it("asks an urgent reason for a second planner's signature: approver and password", async () => {
    const onConfirm = vi.fn();
    render(
      <FrozenWeekBreakInDialog
        open
        onOpenChange={vi.fn()}
        weekStart="2026-10-18"
        teamId={3}
        onConfirm={onConfirm}
      />,
    );
    await screen.findByRole("option", { name: "Emergency" });
    await screen.findByRole("option", { name: "K. Benali" });
    // Approvers of the crew's site, not the user administration list.
    expect(planning.listBreakInApprovers).toHaveBeenCalledWith(3, null);
    const confirm = screen.getByRole("button", { name: "Commit as break-in" });
    expect(confirm).toBeDisabled();

    const [reasonSelect, approverSelect] = screen.getAllByRole("combobox");
    if (!reasonSelect || !approverSelect) throw new Error("selects not rendered");
    fireEvent.change(reasonSelect, { target: { value: "emergency" } });
    expect(screen.getByText(enPlanning.breakIns.urgentHint)).toBeInTheDocument();
    expect(confirm).toBeDisabled();

    // A dangerous-override reason no longer stands in for the signature.
    fireEvent.change(screen.getByLabelText(enPlanning.breakIns.dangerousOverrideReason), {
      target: { value: "Pump seized on line 2" },
    });
    expect(confirm).toBeDisabled();

    fireEvent.change(approverSelect, { target: { value: "58" } });
    expect(confirm).toBeDisabled();
    const password = screen.getByLabelText(enPlanning.breakIns.approverPassword);
    expect(password).toHaveAttribute("type", "password");
    fireEvent.change(password, { target: { value: "s3cret!" } });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith({
      reason: "emergency",
      approved_by_user_id: 58,
      approver_password: "s3cret!",
      dangerous_override_reason: "Pump seized on line 2",
    });
  });

  it("lets a non-urgent reason in without a signature", async () => {
    const onConfirm = vi.fn();
    render(
      <FrozenWeekBreakInDialog
        open
        onOpenChange={vi.fn()}
        weekStart="2026-10-18"
        teamId={3}
        onConfirm={onConfirm}
      />,
    );
    await screen.findByRole("option", { name: "Other" });
    const [reasonSelect] = screen.getAllByRole("combobox");
    if (!reasonSelect) throw new Error("reason select not rendered");
    fireEvent.change(reasonSelect, { target: { value: "other" } });
    expect(screen.queryByLabelText(enPlanning.breakIns.approverPassword)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Commit as break-in" }));
    expect(onConfirm).toHaveBeenCalledWith({
      reason: "other",
      approved_by_user_id: null,
      approver_password: null,
      dangerous_override_reason: null,
    });
  });

  it("says when nobody else may sign at the crew's site", async () => {
    planning.listBreakInApprovers.mockResolvedValue([]);
    render(
      <FrozenWeekBreakInDialog
        open
        onOpenChange={vi.fn()}
        weekStart="2026-10-18"
        teamId={3}
        onConfirm={vi.fn()}
      />,
    );
    expect(await screen.findByText(enPlanning.breakIns.noApprovers)).toBeInTheDocument();
  });
});

describe("WeeklySchedulePanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    planning.listCrewsWithoutPlanningSite.mockResolvedValue([
      { team_id: 8, team_code: "ELEC", team_name: "Électricité" },
    ]);
    planning.getWeeklySchedule.mockResolvedValue(weekView());
    planning.getPlanningKpis.mockResolvedValue(kpisFixture);
  });

  it("shows the site's KPIs against the frozen baseline and flags weeks frozen without approval", async () => {
    render(<WeeklySchedulePanel />);
    const kpis = await screen.findByRole("region", { name: enPlanning.kpis.title });
    expect(planning.getPlanningKpis).toHaveBeenCalledWith(
      expect.objectContaining({ site_id: 10, to_day: "2026-10-18", exclude_unapproved: false }),
    );
    await waitFor(() => expect(kpis).toHaveTextContent("40 %"));
    expect(kpis).toHaveTextContent("20 %");
    expect(kpis).toHaveTextContent(enPlanning.kpis.notFrozen);
    expect(kpis).toHaveTextContent(enPlanning.weekly.frozenWithoutApproval);
    expect(kpis).toHaveTextContent("1 later released or cancelled");
    expect(kpis).toHaveTextContent(enPlanning.kpis.reopened);
  });

  it("shows the crews' week and marks a week frozen without approval", async () => {
    render(<WeeklySchedulePanel />);
    expect(await screen.findByText("OT-0005")).toBeInTheDocument();
    expect(planning.getWeeklySchedule).toHaveBeenCalledWith(10, expect.any(String));
    expect(screen.getByText("MEC — Mécanique")).toBeInTheDocument();
    expect(screen.getAllByText(enPlanning.weekly.frozenWithoutApproval).length).toBeGreaterThan(0);
    expect(screen.getByText(enPlanning.weekly.frozenWithoutApprovalHint)).toBeInTheDocument();
    expect(screen.getByText(/Frozen automatically · System/)).toBeInTheDocument();
    // Frozen: no freeze action; approval after the fact and the unfreeze request are offered.
    expect(screen.queryByRole("button", { name: enPlanning.weekly.freeze })).toBeNull();
    expect(screen.getByRole("button", { name: enPlanning.weekly.publish })).toBeInTheDocument();
  });

  it("approves after the fact against the row version it showed", async () => {
    planning.approveWeeklySchedule.mockResolvedValue(weekView({ awaiting_approval: false }));
    render(<WeeklySchedulePanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: enPlanning.weekly.approveAfterFreeze }),
    );
    await waitFor(() =>
      expect(planning.approveWeeklySchedule).toHaveBeenCalledWith(
        expect.objectContaining({ site_id: 10, expected_row_version: 7 }),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText(enPlanning.weekly.frozenWithoutApprovalHint)).toBeNull(),
    );
  });

  it("sends an unfreeze request only with a reason", async () => {
    planning.requestWeeklyUnfreeze.mockResolvedValue(weekView());
    render(<WeeklySchedulePanel />);
    fireEvent.click(await screen.findByRole("button", { name: enPlanning.weekly.requestUnfreeze }));
    const send = screen.getByRole("button", { name: enPlanning.weekly.sendUnfreezeRequest });
    expect(send).toBeDisabled();
    fireEvent.change(screen.getByLabelText(enPlanning.weekly.unfreezeReason), {
      target: { value: "Shutdown moved by production" },
    });
    fireEvent.click(send);
    await waitFor(() =>
      expect(planning.requestWeeklyUnfreeze).toHaveBeenCalledWith(
        expect.objectContaining({
          site_id: 10,
          expected_row_version: 7,
          reason: "Shutdown moved by production",
        }),
      ),
    );
  });
});

describe("CrewsWithoutSiteBanner", () => {
  beforeEach(() => vi.clearAllMocks());

  it("names the crews on no weekly schedule and links to where they are fixed", async () => {
    planning.listCrewsWithoutPlanningSite.mockResolvedValue([
      { team_id: 8, team_code: "ELEC", team_name: "Électricité" },
    ]);
    render(
      <MemoryRouter>
        <CrewsWithoutSiteBanner />
      </MemoryRouter>,
    );
    expect(await screen.findByText(enPlanning.crewsWithoutSite.title)).toBeInTheDocument();
    expect(screen.getByText("ELEC — Électricité")).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: enPlanning.crewsWithoutSite.fixTeams }),
    ).toHaveAttribute("href", "/personnel?tab=teams");
    expect(screen.getByRole("link", { name: enPlanning.crewsWithoutSite.fixOrg })).toHaveAttribute(
      "href",
      "/org",
    );
  });

  it("shows nothing when every crew has a planning site", async () => {
    planning.listCrewsWithoutPlanningSite.mockResolvedValue([]);
    const { container } = render(
      <MemoryRouter>
        <CrewsWithoutSiteBanner />
      </MemoryRouter>,
    );
    await waitFor(() => expect(planning.listCrewsWithoutPlanningSite).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });
});

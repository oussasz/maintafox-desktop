/**
 * Planning four-eyes, UI side (review round 2, deviation e): a break-in refused on a PC that
 * received it reads "refused (sync)" with its reason, and the approver's password is never kept
 * for the retry after an assignee override (review N7).
 */
import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import frPlanning from "@/i18n/locale-data/fr/planning.json";
import type { CreateScheduleCommitmentInput, ScheduleBreakIn } from "@shared/ipc-types";

import { BreakInDialog } from "../BreakInDialog";
import { overrideRetryInput } from "../planning-model";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPlanning,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => lookup(key) ?? key,
    i18n: { language: "en" },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn(() => Promise.resolve([])),
}));

vi.mock("@/services/personnel-service", () => ({
  listTeamMembers: vi.fn(() => Promise.resolve([])),
}));

vi.mock("@/services/planning-service", () => ({
  listAssigneeCapacity: vi.fn(() => Promise.resolve([])),
  listBreakInApprovers: vi.fn(() => Promise.resolve([])),
}));

function breakIn(overrides: Partial<ScheduleBreakIn>): ScheduleBreakIn {
  return {
    id: 1,
    schedule_commitment_id: 10,
    break_in_reason: "emergency",
    approved_by_user_id: 58,
    approved_by_personnel_id: 7,
    override_reason: null,
    kind: "move",
    old_slot_start: "2026-05-05T08:00:00Z",
    old_slot_end: "2026-05-05T09:00:00Z",
    new_slot_start: "2026-05-06T08:00:00Z",
    new_slot_end: "2026-05-06T09:00:00Z",
    old_assignee_id: null,
    new_assignee_id: null,
    cost_impact_delta: null,
    notification_dedupe_key: null,
    row_version: 1,
    created_by_id: 1,
    created_at: "2026-05-05T07:00:00Z",
    commitment_source_code: "OT-0042",
    approved_by_display_name: "K. Benali",
    old_assignee_name: null,
    new_assignee_name: null,
    approver_signed_at: "2026-05-05T07:00:00Z",
    forced_checks: [],
    old_team_id: 3,
    new_team_id: 3,
    sync_refused_at: null,
    sync_refused_reason: null,
    ...overrides,
  };
}

describe("break-ins refused on a PC that received them", () => {
  it("reads 'refused (sync)' with the reason, and only on the refused one", () => {
    const reason = "refusé (sync) : approbateur sans droit sur le site — re-planned here";
    render(
      <BreakInDialog
        open
        onOpenChange={vi.fn()}
        commitments={[]}
        breakIns={[
          breakIn({ id: 1, commitment_source_code: "OT-0042" }),
          breakIn({
            id: 2,
            commitment_source_code: "OT-0043",
            sync_refused_at: "2026-05-06T12:00:00Z",
            sync_refused_reason: reason,
          }),
        ]}
        teamId={3}
        personnel={[]}
        saving={false}
        onSubmit={vi.fn()}
      />,
    );
    const refused = screen.getByText(/OT-0043/).closest("div");
    const valid = screen.getByText(/OT-0042/).closest("div");
    if (!refused || !valid) throw new Error("break-in rows not rendered");
    const label = within(refused).getByTitle(reason);
    expect(label).toHaveTextContent(enPlanning.breakIns.syncRefused);
    expect(valid).not.toHaveTextContent(enPlanning.breakIns.syncRefused);
    // The French label is the one the Advisor asked for.
    expect(frPlanning.breakIns.syncRefused).toBe("refusé (sync)");
  });
});

describe("the retry after an assignee override", () => {
  const base: CreateScheduleCommitmentInput = {
    schedule_candidate_id: 5,
    expected_candidate_row_version: 2,
    committed_start: "2026-05-06T08:00:00Z",
    committed_end: "2026-05-06T09:00:00Z",
    assigned_team_id: 3,
    assigned_personnel_id: 12,
  } as CreateScheduleCommitmentInput;

  it("never keeps the approver's password: the frozen-week break-in is signed again", () => {
    const signed = {
      ...base,
      frozen_week_break_in: {
        reason: "emergency",
        approved_by_user_id: 58,
        approver_password: "s3cret!",
        dangerous_override_reason: null,
      },
    } as CreateScheduleCommitmentInput;
    const retry = overrideRetryInput(signed);
    expect(retry.frozen_week_break_in).toBeNull();
    expect(JSON.stringify(retry)).not.toContain("s3cret!");
    // Everything else of the commit is kept for the retry.
    expect({ ...retry, frozen_week_break_in: undefined }).toEqual({
      ...signed,
      frozen_week_break_in: undefined,
    });
    // The original input is not changed.
    expect(signed.frozen_week_break_in?.approver_password).toBe("s3cret!");
  });

  it("keeps an input without a break-in as it is", () => {
    expect(overrideRetryInput(base)).toBe(base);
  });
});

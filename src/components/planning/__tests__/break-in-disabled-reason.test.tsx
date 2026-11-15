/**
 * The break-in buttons never stay grey in silence: the sentence beside them names what is missing
 * (the commitment, the reason, the approver, their password) and disappears once it is given.
 */
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import enPlanning from "@/i18n/locale-data/en/planning.json";
import type { ScheduleCommitment } from "@shared/ipc-types";

import { BreakInDialog } from "../BreakInDialog";
import { breakInDisabledReasonKey } from "../BreakInSignerFields";

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

describe("breakInDisabledReasonKey", () => {
  const base = {
    hasTarget: true,
    targetKey: "breakIns.disabled.noReason" as const,
    required: false,
    approverUserId: null,
    password: "",
  };

  it("names the first missing item, in the order of the form", () => {
    expect(breakInDisabledReasonKey({ ...base, hasTarget: false })).toBe(
      "breakIns.disabled.noReason",
    );
    expect(breakInDisabledReasonKey({ ...base, saving: true })).toBe("breakIns.disabled.saving");
    expect(breakInDisabledReasonKey({ ...base, required: true })).toBe(
      "breakIns.disabled.noApprover",
    );
    expect(breakInDisabledReasonKey({ ...base, required: true, approverUserId: 5 })).toBe(
      "breakIns.disabled.noPassword",
    );
  });

  it("is null when the button is available", () => {
    expect(breakInDisabledReasonKey(base)).toBeNull();
    expect(
      breakInDisabledReasonKey({ ...base, required: true, approverUserId: 5, password: "x" }),
    ).toBeNull();
  });
});

describe("BreakInDialog disabled reason", () => {
  const commitment = {
    id: 10,
    row_version: 1,
    committed_start: "2026-05-05T08:00:00Z",
    committed_end: "2026-05-05T09:00:00Z",
    assigned_team_id: 3,
    assigned_personnel_id: null,
  } as unknown as ScheduleCommitment;

  function renderDialog(initialCommitmentId: number | null) {
    return render(
      <BreakInDialog
        open
        onOpenChange={vi.fn()}
        commitments={[commitment]}
        breakIns={[]}
        teamId={3}
        initialCommitmentId={initialCommitmentId}
        personnel={[]}
        saving={false}
        onSubmit={vi.fn()}
      />,
    );
  }

  it("says to choose the commitment while none is chosen", () => {
    renderDialog(null);
    expect(screen.getByTestId("break-in-disabled-reason")).toHaveTextContent(
      "Choose the commitment to move.",
    );
    expect(screen.getByRole("button", { name: enPlanning.breakIns.create })).toBeDisabled();
  });

  it("shows no reason once the commitment is chosen", () => {
    renderDialog(10);
    expect(screen.queryByTestId("break-in-disabled-reason")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: enPlanning.breakIns.create })).toBeEnabled();
  });
});

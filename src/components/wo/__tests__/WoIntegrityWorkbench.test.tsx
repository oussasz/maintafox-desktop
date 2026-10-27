/**
 * AU-116: the second person offered for waiving an error finding comes from the backend's waiver
 * rule (`list_integrity_waiver_approvers`), not from the work-order people list.
 */
import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DataIntegrityFindingRow } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({ formatNumber: (n: number) => String(n) }),
}));
vi.mock("@/components/ui/select", () => ({
  Select: ({ value, children }: { value: string; children: ReactNode }) => (
    <select data-testid="select" value={value} onChange={() => undefined}>
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

const svc = vi.hoisted(() => ({
  listDataIntegrityFindings: vi.fn(),
  listIntegrityWaiverApprovers: vi.fn(),
  runDataIntegrityDetectors: vi.fn(),
  waiveDataIntegrityFinding: vi.fn(),
  applyDataIntegrityRepair: vi.fn(),
}));
vi.mock("@/services/data-integrity-service", () => svc);
vi.mock("@/services/analytics-contract-service", () => ({
  listAnalyticsContractVersions: vi.fn().mockResolvedValue([]),
}));
// The work-order people list must no longer feed the picker.
const people = vi.hoisted(() => ({ listWoLaborCandidates: vi.fn() }));
vi.mock("@/services/wo-execution-service", () => people);

import { WoIntegrityWorkbench } from "../WoIntegrityWorkbench";

const ERROR_FINDING: DataIntegrityFindingRow = {
  id: 1,
  entity_sync_id: "f-1",
  row_version: 1,
  severity: "error",
  domain: "wo",
  record_class: "work_order_downtime_segments",
  record_id: 4,
  finding_code: "WO_DOWNTIME_NEGATIVE_DURATION",
  details_json: "{}",
  detected_at: "2026-10-27T08:00:00Z",
  cleared_at: null,
  status: "open",
  waiver_reason: null,
  waiver_approver_id: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  svc.listDataIntegrityFindings.mockResolvedValue([ERROR_FINDING]);
});

/** The approver picker: the row's select that offers "no approver" (the repair select does not). */
function approverSelect(): HTMLSelectElement {
  const found = screen
    .getAllByTestId("select")
    .filter((s): s is HTMLSelectElement => s.querySelector('option[value="__none"]') !== null);
  expect(found).toHaveLength(1);
  const [select] = found;
  if (!select) throw new Error("approver picker not rendered");
  return select;
}

function optionLabels(): string[] {
  return Array.from(approverSelect().querySelectorAll("option")).map((o) => o.textContent ?? "");
}

describe("WoIntegrityWorkbench waiver approver picker", () => {
  it("offers exactly the people the waiver rule returns", async () => {
    svc.listIntegrityWaiverApprovers.mockResolvedValue([
      { user_id: 7, display_name: "Amina Supervisor" },
      { user_id: 9, display_name: "Karim Admin" },
    ]);
    render(<WoIntegrityWorkbench />);
    await waitFor(() => expect(optionLabels()).toContain("Karim Admin"));
    expect(optionLabels()).toEqual(["integrity.approverNone", "Amina Supervisor", "Karim Admin"]);
    const values = Array.from(approverSelect().querySelectorAll("option")).map((o) =>
      o.getAttribute("value"),
    );
    expect(values).toEqual(["__none", "7", "9"]);
    expect(people.listWoLaborCandidates).not.toHaveBeenCalled();
    expect(screen.queryByText("integrity.approverNoneAvailable")).toBeNull();
  });

  it("says no one can approve when the rule returns nobody", async () => {
    svc.listIntegrityWaiverApprovers.mockResolvedValue([]);
    render(<WoIntegrityWorkbench />);
    expect(await screen.findByText("integrity.approverNoneAvailable")).toBeInTheDocument();
    expect(optionLabels()).toEqual(["integrity.approverNone"]);
  });

  it("says no one can approve when the list cannot be read", async () => {
    svc.listIntegrityWaiverApprovers.mockRejectedValue(new Error("refused"));
    render(<WoIntegrityWorkbench />);
    expect(await screen.findByText("integrity.approverNoneAvailable")).toBeInTheDocument();
    expect(optionLabels()).toEqual(["integrity.approverNone"]);
  });
});

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import { PermitsPage } from "@/pages/PermitsPage";
import type {
  PermitIsolationRecord,
  PermitTypeRecord,
  WorkPermitRecord,
} from "@/services/permit-service";

// Resolve keys against the real English catalogue so a missing key fails the test.
function translate(key: string, opts?: Record<string, unknown>): string {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enPermits,
    );
  if (typeof value !== "string") throw new Error(`missing permits i18n key: ${key}`);
  return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate, i18n: { language: "en" } }),
}));

const svc = vi.hoisted(() => ({
  listPermitTypes: vi.fn(),
  listWorkPermits: vi.fn(),
  permitComplianceKpi30d: vi.fn(),
  listOpenPermitsReport: vi.fn(),
  listPermitIsolations: vi.fn(),
  listPermitSuspensions: vi.fn(),
  listPermitHandoverLogs: vi.fn(),
  getLotoCardView: vi.fn(),
  recordLotoCardPrint: vi.fn(),
  setWorkPermitStatus: vi.fn(),
  suspendWorkPermit: vi.fn(),
  updateWorkPermit: vi.fn(),
  upsertPermitIsolation: vi.fn(),
  applyPermitIsolation: vi.fn(),
  verifyPermitIsolation: vi.fn(),
  verifyPermitIsolationRemoval: vi.fn(),
  appendPermitHandoverLog: vi.fn(),
  createWorkPermit: vi.fn(),
  upsertPermitType: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const getWo = vi.hoisted(() => vi.fn());
vi.mock("@/services/wo-service", () => ({ getWo }));
vi.mock("@/services/asset-search-service", () => ({
  searchAssets: vi.fn().mockResolvedValue([]),
}));

const session = vi.hoisted(() => ({ userId: 7 }));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: session.userId } }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const LOTO_TYPE: PermitTypeRecord = {
  id: 1,
  entity_sync_id: "t1",
  code: "loto",
  name: "LOTO",
  description: "Lockout/Tagout",
  requires_hse_approval: true,
  requires_operations_approval: true,
  requires_atmospheric_test: false,
  max_duration_hours: 24,
  mandatory_ppe_ids_json: "[]",
  mandatory_control_rules_json: "{}",
  row_version: 1,
};

function permit(overrides: Partial<WorkPermitRecord> = {}): WorkPermitRecord {
  return {
    id: 41,
    entity_sync_id: "p41",
    code: "PTW-2026-000041",
    linked_work_order_id: 5,
    permit_type_id: 1,
    asset_id: 900,
    entity_id: 3,
    status: "issued",
    requested_at: "2026-10-10T08:00:00Z",
    issued_at: "2026-10-10T08:30:00Z",
    activated_at: null,
    expires_at: "2099-01-01T00:00:00Z",
    closed_at: null,
    handed_back_at: null,
    row_version: 4,
    permit_type_code: "loto",
    permit_type_name: "LOTO",
    asset_code: "PMP-101",
    asset_label: "Feed pump",
    linked_work_order_code: "OT-0042",
    ...overrides,
  };
}

/** Applied by user 7, not yet verified. */
function isolation(overrides: Partial<PermitIsolationRecord> = {}): PermitIsolationRecord {
  return {
    id: 300,
    entity_sync_id: "i300",
    permit_id: 41,
    isolation_point: "MCC-1 breaker",
    energy_type: "electrical",
    isolation_method: "breaker lock-out",
    lock_number: "L-12",
    applied_by_id: 7,
    verified_by_id: null,
    applied_at: "2026-10-10T09:00:00Z",
    verified_at: null,
    removal_verified_at: null,
    removal_verified_by_id: null,
    row_version: 2,
    applied_by_name: "Amine Tech",
    verified_by_name: null,
    removal_verified_by_name: null,
    ...overrides,
  };
}

const VERIFIED = {
  verified_by_id: 8,
  verified_at: "2026-10-10T09:10:00Z",
  verified_by_name: "Sara Sup",
};

function renderPage(path = "/permits") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <PermitsPage />
    </MemoryRouter>,
  );
}

async function openPermit(code = "PTW-2026-000041") {
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: `Select ${code}` }));
  return screen.findByRole("region", { name: `Permit ${code}` });
}

describe("PermitsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 7;
    svc.listPermitTypes.mockResolvedValue([LOTO_TYPE]);
    svc.listWorkPermits.mockResolvedValue([permit()]);
    svc.permitComplianceKpi30d.mockResolvedValue({
      activated_count: 0,
      handed_back_on_time_count: 0,
      rate: null,
    });
    svc.listOpenPermitsReport.mockResolvedValue([]);
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitIsolations.mockResolvedValue([isolation()]);
  });

  it("lists permits with business labels instead of internal ids", async () => {
    renderPage();
    const button = await screen.findByRole("button", { name: "Select PTW-2026-000041" });
    const row = within(button.closest("tr") as HTMLElement);
    expect(row.getByText("PMP-101 — Feed pump")).toBeInTheDocument();
    expect(row.getByText("OT-0042")).toBeInTheDocument();
    expect(row.getByText("LOTO")).toBeInTheDocument();
    expect(row.queryByText("900")).toBeNull();
  });

  it("does not let the applier verify their own isolation, and blocks activation", async () => {
    const panel = await openPermit();
    expect(await within(panel).findByRole("button", { name: "Verify" })).toBeDisabled();
    expect(within(panel).getAllByText(enPermits.isolation.selfBlocked).length).toBeGreaterThan(0);
    expect(within(panel).getByRole("button", { name: "Activate" })).toBeDisabled();
    expect(within(panel).getByText(enPermits.gate.unverified)).toBeInTheDocument();
  });

  it("lets a second person verify, then activates the permit", async () => {
    session.userId = 8;
    svc.verifyPermitIsolation.mockImplementation(async () => {
      // The panel reloads isolations after the write.
      svc.listPermitIsolations.mockResolvedValue([isolation({ ...VERIFIED, row_version: 3 })]);
      return isolation({ ...VERIFIED, row_version: 3 });
    });
    svc.setWorkPermitStatus.mockResolvedValue(permit({ status: "active", row_version: 5 }));

    const panel = await openPermit();
    fireEvent.click(await within(panel).findByRole("button", { name: "Verify" }));
    await waitFor(() =>
      expect(svc.verifyPermitIsolation).toHaveBeenCalledWith({
        isolation_id: 300,
        expected_row_version: 2,
        lock_number: null,
      }),
    );

    await waitFor(() =>
      expect(within(panel).getByRole("button", { name: "Activate" })).toBeEnabled(),
    );
    fireEvent.click(within(panel).getByRole("button", { name: "Activate" }));
    await waitFor(() =>
      expect(svc.setWorkPermitStatus).toHaveBeenCalledWith({
        id: 41,
        status: "active",
        expected_row_version: 4,
      }),
    );
  });

  it("blocks closing until every lock removal is verified", async () => {
    session.userId = 8;
    svc.listWorkPermits.mockResolvedValue([permit({ status: "active", row_version: 5 })]);
    svc.listPermitIsolations.mockResolvedValue([isolation({ ...VERIFIED, verified_by_id: 9 })]);
    svc.verifyPermitIsolationRemoval.mockImplementation(async () => {
      const removed = isolation({
        ...VERIFIED,
        verified_by_id: 9,
        removal_verified_at: "2026-10-10T15:00:00Z",
        removal_verified_by_id: 8,
      });
      svc.listPermitIsolations.mockResolvedValue([removed]);
      return removed;
    });

    const panel = await openPermit();
    expect(await within(panel).findByRole("button", { name: "Close permit" })).toBeDisabled();
    expect(within(panel).getByText(enPermits.gate.pendingRemoval)).toBeInTheDocument();

    fireEvent.click(within(panel).getByRole("button", { name: "Verify removal" }));
    await waitFor(() => expect(svc.verifyPermitIsolationRemoval).toHaveBeenCalled());
    await waitFor(() =>
      expect(within(panel).getByRole("button", { name: "Close permit" })).toBeEnabled(),
    );
  });

  it("opens the create dialog pre-linked to the work order from the WO launch point", async () => {
    svc.listWorkPermits.mockResolvedValue([]);
    getWo.mockResolvedValue({
      wo: { id: 5, code: "OT-0042", asset_code: "PMP-101", asset_label: "Feed pump" },
      transitions: [],
    });
    renderPage("/permits?createForWo=5");
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(enPermits.create.title)).toBeInTheDocument();
    expect(getWo).toHaveBeenCalledWith(5);
    expect(within(dialog).getByText(/OT-0042/)).toBeInTheDocument();
    expect(within(dialog).getByText("PMP-101 — Feed pump")).toBeInTheDocument();

    // Submitting without a permit type is rejected client-side and never reaches IPC.
    fireEvent.click(within(dialog).getByRole("button", { name: enPermits.create.submit }));
    expect(await within(dialog).findByText(enPermits.errors.required)).toBeInTheDocument();
    expect(svc.createWorkPermit).not.toHaveBeenCalled();
  });
});

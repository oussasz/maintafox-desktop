import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import enTraining from "@/i18n/locale-data/en/training.json";
import type {
  PermitHandoverLogRecord,
  PermitIsolationRecord,
  WorkPermitRecord,
} from "@/services/permit-service";

import { PermitDetailPanel } from "../PermitDetailPanel";

// Keys resolve against the real English catalogue so a missing key fails the test.
function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

function fill(text: string, opts: Record<string, unknown> = {}): string {
  return text.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = ns === "permits" ? lookup(enPermits, key) : undefined;
      if (value == null) {
        if (ns === "permits") throw new Error(`missing permits key: ${key}`);
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      }
      return fill(value, opts);
    },
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

// A coded refusal is shown with its translated text (training.json "errors").
vi.mock("@/lib/app-error", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  appErrorReason: (e: { code?: string }) =>
    ((e.code && lookup(enTraining, `errors.${e.code}`)) || "unknown").replace(/\{\{\w+\}\}/g, ""),
}));

vi.mock("@/components/permits/loto-card-print", () => ({ printLotoCard: vi.fn() }));

const svc = vi.hoisted(() => ({
  listPermitIsolations: vi.fn(),
  listPermitIsolationOptions: vi.fn(),
  listPermitSuspensions: vi.fn(),
  listPermitHandoverLogs: vi.fn(),
  listPermitHandoverRoles: vi.fn(),
  listPermitHandoverReceivers: vi.fn(),
  appendPermitHandoverLog: vi.fn(),
  getLotoCardView: vi.fn(),
  recordLotoCardPrint: vi.fn(),
  setWorkPermitStatus: vi.fn(),
  suspendWorkPermit: vi.fn(),
  updateWorkPermit: vi.fn(),
  upsertPermitIsolation: vi.fn(),
  applyPermitIsolation: vi.fn(),
  verifyPermitIsolation: vi.fn(),
  verifyPermitIsolationRemoval: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const session = vi.hoisted(() => ({ userId: 10 }));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({
    info: { user_id: session.userId, username: "u", display_name: "Signed-in user" },
  }),
}));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: () => true }),
}));

function permit(overrides: Partial<WorkPermitRecord> = {}): WorkPermitRecord {
  return {
    id: 41,
    entity_sync_id: "p41",
    code: "PTW-2026-000041",
    linked_work_order_id: null,
    permit_type_id: 1,
    asset_id: 900,
    entity_id: 3,
    status: "active",
    requested_at: "2026-10-10T08:00:00Z",
    issued_at: "2026-10-10T08:30:00Z",
    activated_at: "2026-10-10T09:30:00Z",
    expires_at: "2099-01-01T00:00:00Z",
    closed_at: null,
    handed_back_at: null,
    row_version: 5,
    permit_type_code: "loto",
    permit_type_name: "LOTO",
    asset_code: "PMP-101",
    asset_label: "Feed pump",
    linked_work_order_code: null,
    permit_type_requires_energy_isolation: true,
    permit_type_requires_three_person_removal: true,
    ...overrides,
  };
}

/** Applied by user 7, verified by user 8 (Sara Sup), removal not yet confirmed. */
const VERIFIED_ISOLATION: PermitIsolationRecord = {
  id: 300,
  entity_sync_id: "i300",
  permit_id: 41,
  isolation_point: "MCC-1 breaker",
  energy_type: "",
  isolation_method: "",
  energy_type_id: null,
  energy_type_code: null,
  energy_type_label: null,
  isolation_method_id: null,
  isolation_method_label: null,
  mapping_review_required: false,
  lock_number: "L-12",
  applied_by_id: 7,
  verified_by_id: 8,
  applied_at: "2026-10-10T09:00:00Z",
  verified_at: "2026-10-10T09:10:00Z",
  removal_verified_at: null,
  removal_verified_by_id: null,
  row_version: 3,
  applied_by_name: "Amine Tech",
  verified_by_name: "Sara Sup",
  removal_verified_by_name: null,
};

function renderPanel(p: WorkPermitRecord = permit()) {
  render(
    <MemoryRouter>
      <PermitDetailPanel permit={p} onChanged={vi.fn()} />
    </MemoryRouter>,
  );
  return screen.findByTestId("permit-isolation");
}

const removalButton = (row: HTMLElement) =>
  within(row).getByRole("button", { name: enPermits.isolation.verifyRemoval });

describe("PermitDetailPanel — lock removal with a third person", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 10;
    svc.listPermitIsolations.mockResolvedValue([VERIFIED_ISOLATION]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
  });

  it("names the isolation's verifier as someone who may not confirm the removal", async () => {
    const row = await renderPanel();
    expect(within(row).getByTestId("isolation-three-person")).toHaveTextContent(
      fill(enPermits.isolation.threePersonNotice, { name: "Sara Sup" }),
    );
    // A third person may confirm.
    expect(removalButton(row)).toBeEnabled();
  });

  it("does not let the verifier confirm the removal and says why", async () => {
    session.userId = 8;
    const row = await renderPanel();
    expect(removalButton(row)).toBeDisabled();
    expect(removalButton(row)).toHaveAttribute("title", enPermits.isolation.threePersonSelf);
    expect(within(row).getByTestId("isolation-three-person")).toHaveTextContent(
      enPermits.isolation.threePersonSelf,
    );
  });

  it("shows no third-person notice when the permit type does not ask for it", async () => {
    session.userId = 8;
    const row = await renderPanel(permit({ permit_type_requires_three_person_removal: false }));
    expect(within(row).queryByTestId("isolation-three-person")).toBeNull();
    expect(removalButton(row)).toBeEnabled();
  });

  it("shows the server's refusal in the user's language", async () => {
    svc.verifyPermitIsolationRemoval.mockRejectedValue({ code: "PERMIT_THREE_PERSON_REMOVAL" });
    const row = await renderPanel();
    fireEvent.click(removalButton(row));
    await waitFor(() =>
      expect(svc.verifyPermitIsolationRemoval).toHaveBeenCalledWith({
        isolation_id: 300,
        expected_row_version: 3,
        lock_number: null,
      }),
    );
    expect(
      await screen.findByText(String(lookup(enTraining, "errors.PERMIT_THREE_PERSON_REMOVAL"))),
    ).toBeInTheDocument();
  });
});

describe("PermitDetailPanel — recorded hand-overs", () => {
  const handover = (o: Partial<PermitHandoverLogRecord>): PermitHandoverLogRecord => ({
    id: 1,
    entity_sync_id: "h1",
    permit_id: 41,
    handed_from_role: "MAINT",
    handed_to_role: "OPS",
    confirmation_note: "Zone left clean",
    signed_at: "2026-10-11T08:00:00Z",
    signed_by_id: 7,
    signed_by_name: "Amine Tech",
    received_by_id: 9,
    received_by_name: "Ali Operations",
    row_version: 1,
    ...o,
  });

  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 10;
    svc.listPermitIsolations.mockResolvedValue([]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
    svc.listPermitHandoverRoles.mockResolvedValue([
      { code: "MAINT", label: "Maintenance", review_required: false },
    ]);
    svc.listPermitHandoverLogs.mockResolvedValue([
      handover({}),
      handover({
        id: 2,
        entity_sync_id: "h2",
        signed_by_id: null,
        signed_by_name: null,
        received_by_id: null,
        received_by_name: null,
      }),
    ]);
  });

  it("shows who signed and who received each hand-over, with the role labels", async () => {
    render(
      <MemoryRouter>
        <PermitDetailPanel permit={permit({ status: "closed" })} onChanged={vi.fn()} />
      </MemoryRouter>,
    );
    expect(
      await screen.findByText(
        `${fill(enPermits.handover.signedBy, { name: "Amine Tech" })} · ${fill(
          enPermits.handover.receivedBy,
          { name: "Ali Operations" },
        )}`,
      ),
    ).toBeInTheDocument();
    // An older hand-over without a recorded signer says so instead of inventing one.
    expect(screen.getByText(enPermits.handover.signerNotRecorded)).toBeInTheDocument();
    // Codes of the list are shown with their label; a code no longer listed stays as stored.
    expect((await screen.findAllByText("Maintenance")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("OPS").length).toBe(2);
    // The hand-over form is offered on a closed permit.
    expect(screen.getByRole("form", { name: enPermits.handover.add })).toBeInTheDocument();
  });
});

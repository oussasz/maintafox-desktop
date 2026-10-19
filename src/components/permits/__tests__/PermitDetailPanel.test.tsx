import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import enTraining from "@/i18n/locale-data/en/training.json";
import type {
  PermitHandoverLogRecord,
  PermitIsolationRecord,
  WorkPermitRecord,
} from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

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
  cancelPermitIsolation: vi.fn(),
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

// The rights held at the permit's site (every right unless a test withholds some).
const siteRights = vi.hoisted(() => ({ withheld: new Set<string>() }));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: (p: string) => !siteRights.withheld.has(p) }),
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
  cancelled_at: null,
  cancelled_by_id: null,
  cancelled_by_name: null,
};

function renderPanel(p: WorkPermitRecord = permit()) {
  render(
    <MemoryRouter>
      <PermitDetailPanel permit={p} onChanged={vi.fn()} />
    </MemoryRouter>,
  );
  return screen.findByTestId("permit-isolation");
}

const confirmName = enPermits.isolation.verifyRemoval;
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

  it("asks for a confirmation naming the point, who applied it and who verified it", async () => {
    const row = await renderPanel();
    fireEvent.click(removalButton(row));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(
      fill(enPermits.isolation.removalConfirmDescription, {
        point: "MCC-1 breaker",
        applier: "Amine Tech",
        verifier: "Sara Sup",
      }),
    );
    // Nothing is recorded until the person confirms.
    expect(svc.verifyPermitIsolationRemoval).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: confirmName }));
    await waitFor(() => expect(svc.verifyPermitIsolationRemoval).toHaveBeenCalledTimes(1));
  });

  it("records nothing when the confirmation is cancelled", async () => {
    const row = await renderPanel();
    fireEvent.click(removalButton(row));
    const dialog = await screen.findByRole("dialog");
    // The common-namespace key stands in for the cancel label (not part of the permits catalogue).
    fireEvent.click(within(dialog).getByRole("button", { name: "ds.confirm.cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(svc.verifyPermitIsolationRemoval).not.toHaveBeenCalled();
  });

  it("shows the server's refusal in the user's language", async () => {
    svc.verifyPermitIsolationRemoval.mockRejectedValue({ code: "PERMIT_THREE_PERSON_REMOVAL" });
    const row = await renderPanel();
    fireEvent.click(removalButton(row));
    fireEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: confirmName }),
    );
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
    // The dialog does not stay open over the reason.
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});

describe("PermitDetailPanel — recorded hand-overs", () => {
  const handover = (o: Partial<PermitHandoverLogRecord>): PermitHandoverLogRecord => ({
    id: 1,
    entity_sync_id: "h1",
    permit_id: 41,
    handed_from_role: "MAINT",
    handed_to_role: "OPS",
    handed_from_label: "Mechanical maintenance",
    handed_to_label: null,
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
        // A role since deactivated or left out of the current list keeps the label the server found.
        handed_to_label: "Night shift",
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
    // Roles are shown with the label sent with the hand-over (not the form's current list); a
    // code in no version of the list stays as stored.
    expect(screen.getAllByText("Mechanical maintenance")).toHaveLength(2);
    expect(screen.getByText("Night shift")).toBeInTheDocument();
    expect(screen.getAllByText("OPS")).toHaveLength(1);
    // The hand-over form is offered on a closed permit.
    expect(screen.getByRole("form", { name: enPermits.handover.add })).toBeInTheDocument();
  });
});

describe("PermitDetailPanel — cancelling a planned isolation line", () => {
  const planned: PermitIsolationRecord = {
    ...VERIFIED_ISOLATION,
    id: 310,
    entity_sync_id: "i310",
    isolation_point: "Valve V-7",
    lock_number: null,
    applied_by_id: null,
    verified_by_id: null,
    applied_at: null,
    verified_at: null,
    row_version: 1,
    applied_by_name: null,
    verified_by_name: null,
  };
  const cancelButton = (row: HTMLElement) =>
    within(row).queryByRole("button", { name: enPermits.isolation.cancel });

  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 10;
    siteRights.withheld = new Set();
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
  });

  it("offers it on a line never applied while the permit is a draft or waiting for review", async () => {
    svc.listPermitIsolations.mockResolvedValue([planned]);
    for (const status of ["draft", "pending_review"] as const) {
      const row = await renderPanel(permit({ status }));
      expect(cancelButton(row)).toBeInTheDocument();
      cleanup();
    }
  });

  it("does not offer it on an applied line, past review, or without the issue right", async () => {
    svc.listPermitIsolations.mockResolvedValue([VERIFIED_ISOLATION]);
    let row = await renderPanel(permit({ status: "draft" }));
    expect(cancelButton(row)).toBeNull();
    cleanup();

    svc.listPermitIsolations.mockResolvedValue([planned]);
    for (const status of ["approved", "issued", "suspended"] as const) {
      row = await renderPanel(permit({ status }));
      expect(cancelButton(row)).toBeNull();
      cleanup();
    }

    siteRights.withheld = new Set([P.PTW_ISSUE]);
    row = await renderPanel(permit({ status: "draft" }));
    expect(cancelButton(row)).toBeNull();
  });

  it("confirms naming the line, then cancels it and passes on the permit sent back to draft", async () => {
    svc.listPermitIsolations.mockResolvedValue([planned]);
    const draft = permit({ status: "draft", row_version: 6 });
    svc.cancelPermitIsolation.mockResolvedValue({
      permit: draft,
      isolation: { ...planned, cancelled_at: "2026-10-12T10:00:00Z", cancelled_by_id: 10 },
    });
    const onChanged = vi.fn();
    render(
      <MemoryRouter>
        <PermitDetailPanel permit={permit({ status: "pending_review" })} onChanged={onChanged} />
      </MemoryRouter>,
    );
    const row = await screen.findByTestId("permit-isolation");
    fireEvent.click(cancelButton(row) as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Valve V-7");
    expect(within(dialog).getByTestId("isolation-cancel-reset-notice")).toHaveTextContent(
      enPermits.isolation.cancelResetNotice,
    );
    fireEvent.click(within(dialog).getByRole("button", { name: enPermits.isolation.cancel }));
    await waitFor(() =>
      expect(svc.cancelPermitIsolation).toHaveBeenCalledWith({
        isolation_id: 310,
        expected_row_version: 1,
      }),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledWith(draft));
  });

  it("shows a cancelled line greyed as cancelled, with who and when, and no action", async () => {
    svc.listPermitIsolations.mockResolvedValue([
      {
        ...planned,
        cancelled_at: "2026-10-12T10:00:00Z",
        cancelled_by_id: 8,
        cancelled_by_name: "Sara Sup",
      },
    ]);
    const row = await renderPanel(permit({ status: "draft" }));
    expect(row).toHaveAttribute("data-cancelled", "true");
    expect(row.className).toContain("opacity-60");
    expect(within(row).getByText(enPermits.isolation.state.cancelled)).toBeInTheDocument();
    expect(within(row).getByTestId("isolation-cancelled-by")).toHaveTextContent("Sara Sup");
    expect(within(row).queryAllByRole("button")).toHaveLength(0);
  });

  it("does not count a cancelled line as an isolation left to verify or remove", async () => {
    svc.listPermitIsolations.mockResolvedValue([
      { ...VERIFIED_ISOLATION, removal_verified_at: "2026-10-12T11:00:00Z" },
      { ...planned, cancelled_at: "2026-10-12T10:00:00Z", cancelled_by_name: "Sara Sup" },
    ]);
    render(
      <MemoryRouter>
        <PermitDetailPanel permit={permit({ status: "active" })} onChanged={vi.fn()} />
      </MemoryRouter>,
    );
    await screen.findAllByTestId("permit-isolation");
    expect(screen.queryByText(enPermits.gate.pendingRemoval)).toBeNull();
  });
});

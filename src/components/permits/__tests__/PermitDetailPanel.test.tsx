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
  checkWorkPermitApproval: vi.fn().mockResolvedValue(undefined),
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
  recordPermitIsolationRemoved: vi.fn(),
  forcePermitIsolationRemoval: vi.fn(),
  recordPermitWorkerInformed: vi.fn(),
  listPermitContactMethods: vi.fn(),
  listPermitLockRemovers: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

/** L4-03 / L4-04 / L4-12: the safety content as the server returns it (nothing blocks by default). */
const SAFETY = {
  permit_id: 41,
  permit_status: "active",
  permit_row_version: 5,
  work_description: "Replace the pump seal",
  risk_assessment: "Pressure",
  preventive_measures: "Drain, lock out",
  ppe_required: "Gloves",
  requires_hse_approval: false,
  requires_gas_test: false,
  hse_approved_by_id: null,
  hse_approved_by_name: null,
  hse_approved_at: null,
  hse_approval_note: null,
  hse_self_approved: null,
  gas_validity_hours: 4,
  gas_limits: {
    o2_min_percent: 19.5,
    o2_max_percent: 23.5,
    lel_max_percent: 10,
    h2s_max_ppm: 10,
    co_max_ppm: 25,
  },
  gas_tests: [],
  issue_blockers: [] as { code: string; params: Record<string, string> }[],
};
const safetySvc = vi.hoisted(() => ({
  getPermitSafetyContent: vi.fn(),
  listPermitProposedIsolationPoints: vi.fn(),
  savePermitSafetyContent: vi.fn(),
  recordPermitHseApproval: vi.fn(),
  recordPermitGasTest: vi.fn(),
}));
vi.mock("@/services/permit-safety-service", () => safetySvc);
/** L4-05 / L4-18: the workers of the permit (none by default). */
const workersSvc = vi.hoisted(() => ({
  listPermitWorkers: vi.fn(),
  addPermitWorker: vi.fn(),
  recordPermitWorkerInduction: vi.fn(),
  withdrawPermitWorker: vi.fn(),
  placeMyPermitLock: vi.fn(),
  removeMyPermitLock: vi.fn(),
  forceRemovePermitLock: vi.fn(),
}));
vi.mock("@/services/permit-workers-service", () => workersSvc);
vi.mock("@/services/personnel-service", () => ({
  listPersonnel: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  listExternalCompanies: vi.fn().mockResolvedValue([]),
}));
beforeEach(() => {
  safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
  safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
  workersSvc.listPermitWorkers.mockResolvedValue({
    permit_id: 41,
    permit_status: "active",
    workers: [],
    locks: [],
    open_lock_count: 0,
  });
});

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
    // Four-eyes: written and submitted by Wendy Writer, approved by Alex Approver.
    created_by_id: 20,
    created_by_name: "Wendy Writer",
    submitted_by_id: 20,
    submitted_by_name: "Wendy Writer",
    submitted_at: "2026-10-10T07:00:00Z",
    approved_by_id: 21,
    approved_by_name: "Alex Approver",
    approved_at: "2026-10-10T08:00:00Z",
    self_approved: false,
    cancel_reason: null,
    cancelled_by_id: null,
    cancelled_by_name: null,
    cancelled_at: null,
    ...overrides,
  };
}

/**
 * Applied by user 7, verified by user 8 (Sara Sup), taken off by user 7 ("I removed my lock"),
 * removal not yet confirmed.
 */
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
  // Verified before the zero-energy test was recorded (L4-13): shown "not recorded".
  zero_energy_note: null,
  removal_verified_at: null,
  removal_verified_by_id: null,
  row_version: 3,
  applied_by_name: "Amine Tech",
  verified_by_name: "Sara Sup",
  removal_verified_by_name: null,
  cancelled_at: null,
  cancelled_by_id: null,
  cancelled_by_name: null,
  removed_by_id: 7,
  removed_by_name: "Amine Tech",
  removed_at: "2026-10-10T17:00:00Z",
  forced_removal: false,
  forced_authorized_by_id: null,
  forced_authorized_by_name: null,
  forced_reason: null,
  forced_worker_absent: false,
  forced_contact_attempts: [],
  informed_by_id: null,
  informed_by_name: null,
  informed_at: null,
  informed_method: null,
  informed_in_app: false,
};

/** The same lock still on (not taken off yet). */
const LOCK_ON: PermitIsolationRecord = {
  ...VERIFIED_ISOLATION,
  removed_by_id: null,
  removed_by_name: null,
  removed_at: null,
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
    safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
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

describe("PermitDetailPanel — taking a lock off", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    siteRights.withheld = new Set();
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
    svc.listPermitContactMethods.mockResolvedValue([
      { code: "Téléphone", label: "Téléphone", review_required: true },
    ]);
    svc.listPermitLockRemovers.mockResolvedValue({ items: [], total: 0, empty_reason: null });
  });

  const button = (row: HTMLElement, name: string) => within(row).queryByRole("button", { name });

  it("lets the applier record that they took their lock off, before any removal check", async () => {
    session.userId = 7;
    svc.listPermitIsolations.mockResolvedValue([LOCK_ON]);
    svc.recordPermitIsolationRemoved.mockResolvedValue(VERIFIED_ISOLATION);
    const row = await renderPanel();
    expect(button(row, enPermits.isolation.verifyRemoval)).toBeNull();
    expect(button(row, enPermits.forced.action)).toBeNull();
    fireEvent.click(button(row, enPermits.isolation.takeOff) as HTMLElement);
    // A confirmation first, saying the lock then no longer protects the work.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(
      fill(enPermits.isolation.takeOffConfirmDescription, { point: "MCC-1 breaker" }),
    );
    expect(svc.recordPermitIsolationRemoved).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: enPermits.isolation.takeOff }));
    await waitFor(() =>
      expect(svc.recordPermitIsolationRemoved).toHaveBeenCalledWith({
        isolation_id: 300,
        expected_row_version: 3,
        lock_number: null,
      }),
    );
  });

  it("records nothing when the take-off confirmation is cancelled", async () => {
    session.userId = 7;
    svc.listPermitIsolations.mockResolvedValue([LOCK_ON]);
    const row = await renderPanel();
    fireEvent.click(button(row, enPermits.isolation.takeOff) as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "ds.confirm.cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(svc.recordPermitIsolationRemoved).not.toHaveBeenCalled();
  });

  it("offers a forced removal to a close-right holder while the lock is on, and waits for the applier", async () => {
    session.userId = 10;
    svc.listPermitIsolations.mockResolvedValue([LOCK_ON]);
    const row = await renderPanel();
    expect(button(row, enPermits.isolation.takeOff)).toBeNull();
    expect(button(row, enPermits.isolation.verifyRemoval)).toBeNull();
    expect(within(row).getByTestId("isolation-awaiting-take-off")).toHaveTextContent("Amine Tech");
    fireEvent.click(button(row, enPermits.forced.action) as HTMLElement);
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(enPermits.forced.title);
    // Nothing is sent without the remover, the reason, the attestation and a complete attempt.
    fireEvent.click(within(dialog).getByRole("button", { name: enPermits.forced.submit }));
    expect(await within(dialog).findByText(enPermits.forced.attestRequired)).toBeInTheDocument();
    expect(svc.forcePermitIsolationRemoval).not.toHaveBeenCalled();
  });

  it("offers no forced removal without the close right at the permit's site", async () => {
    session.userId = 10;
    siteRights.withheld = new Set([P.PTW_CLOSE]);
    svc.listPermitIsolations.mockResolvedValue([LOCK_ON]);
    const row = await renderPanel();
    expect(button(row, enPermits.forced.action)).toBeNull();
  });

  it("does not let the person who took the lock off verify its removal", async () => {
    session.userId = 12;
    svc.listPermitIsolations.mockResolvedValue([
      { ...VERIFIED_ISOLATION, removed_by_id: 12, removed_by_name: "Rémi Remover" },
    ]);
    const row = await renderPanel(permit({ permit_type_requires_three_person_removal: false }));
    expect(removalButton(row)).toBeDisabled();
    expect(removalButton(row)).toHaveAttribute("title", enPermits.isolation.removerBlocked);
  });

  it("shows a forced removal with who authorised it, why, the attempts, and lets a holder record the worker told", async () => {
    session.userId = 10;
    svc.listPermitIsolations.mockResolvedValue([
      {
        ...VERIFIED_ISOLATION,
        removed_by_id: 12,
        removed_by_name: "Rémi Remover",
        forced_removal: true,
        forced_authorized_by_id: 11,
        forced_authorized_by_name: "Chef Site",
        forced_reason: "Fin de poste, travailleur parti",
        forced_worker_absent: true,
        forced_contact_attempts: [
          { at: "2026-10-10T16:30:00Z", method: "Téléphone", result: "Pas de réponse" },
        ],
      },
    ]);
    const row = await renderPanel();
    const forced = within(row).getByTestId("isolation-forced-removal");
    expect(forced).toHaveTextContent(enPermits.forced.chip);
    expect(forced).toHaveTextContent("Chef Site");
    expect(forced).toHaveTextContent("Fin de poste, travailleur parti");
    expect(forced).toHaveTextContent("Pas de réponse");
    expect(within(row).getByTestId("isolation-worker-informed")).toHaveTextContent(
      enPermits.informed.pending,
    );
    fireEvent.click(button(row, enPermits.informed.action) as HTMLElement);
    expect(await screen.findByRole("dialog")).toHaveTextContent(enPermits.informed.title);
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
    removed_by_id: null,
    removed_by_name: null,
    removed_at: null,
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
    safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
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

describe("PermitDetailPanel — four-eyes approval", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 20;
    svc.listPermitIsolations.mockResolvedValue([]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
    safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
  });

  const show = async (p: WorkPermitRecord) => {
    render(
      <MemoryRouter>
        <PermitDetailPanel permit={p} onChanged={vi.fn()} />
      </MemoryRouter>,
    );
    await waitFor(() => expect(svc.listPermitIsolations).toHaveBeenCalled());
  };

  it("shows who wrote, submitted and approved the permit", async () => {
    await show(permit());
    expect(screen.getByText(enPermits.detail.createdBy)).toBeInTheDocument();
    // Written by, with the date the permit was requested (its creation).
    expect(screen.getByText(enPermits.detail.createdBy).nextElementSibling).toHaveTextContent(
      "Wendy Writer",
    );
    expect(screen.getByText(enPermits.detail.submittedBy)).toBeInTheDocument();
    const approver = screen.getByTestId("permit-approver");
    expect(approver).toHaveTextContent("Alex Approver");
    expect(approver).not.toHaveTextContent(enPermits.detail.selfApproved);
  });

  it("flags a self-approval made on a site without four-eyes", async () => {
    await show(
      permit({ approved_by_id: 20, approved_by_name: "Wendy Writer", self_approved: true }),
    );
    const approver = screen.getByTestId("permit-approver");
    expect(approver).toHaveTextContent("Wendy Writer");
    expect(within(approver).getByText(enPermits.detail.selfApproved)).toBeInTheDocument();
    // The explanation is visible text (reachable by keyboard, touch and screen readers).
    expect(within(approver).getByText(enPermits.detail.selfApprovedHint)).toBeVisible();
  });

  it("says the approver is not recorded on a permit approved before it was recorded", async () => {
    await show(
      permit({
        created_by_id: null,
        created_by_name: null,
        submitted_by_id: null,
        submitted_by_name: null,
        submitted_at: null,
        approved_by_id: null,
        approved_by_name: null,
        approved_at: null,
      }),
    );
    expect(screen.getByTestId("permit-approver")).toHaveTextContent(
      enPermits.detail.approverNotRecorded,
    );
    expect(screen.getByText(enPermits.detail.approverNotRecordedHint)).toBeVisible();
    expect(screen.queryByText(enPermits.detail.createdBy)).toBeNull();
  });

  it("shows no approver line on a permit still waiting for review", async () => {
    await show(
      permit({
        status: "pending_review",
        approved_by_id: null,
        approved_by_name: null,
        approved_at: null,
      }),
    );
    expect(screen.queryByTestId("permit-approver")).toBeNull();
  });

  it("shows the red expired-permit alert while an open permit is past its expiry (L4-08)", async () => {
    await show(permit({ status: "active", expires_at: "2020-01-01T00:00:00Z" }));
    const alert = await screen.findByTestId("permit-overdue");
    expect(alert).toHaveTextContent(enPermits.overdue.title);
  });

  it("shows no expired-permit alert once the permit is handed back", async () => {
    await show(permit({ status: "handed_back", expires_at: "2020-01-01T00:00:00Z" }));
    expect(await screen.findByTestId("permit-status")).toBeInTheDocument();
    expect(screen.queryByTestId("permit-overdue")).toBeNull();
  });

  it("greys the approve step with the server's four-eyes reason before any click (L4-14)", async () => {
    svc.checkWorkPermitApproval.mockRejectedValueOnce({ code: "PERMIT_APPROVER_IS_SUBMITTER" });
    const p = permit({
      status: "pending_review",
      approved_by_id: null,
      approved_by_name: null,
      approved_at: null,
    });
    await show(p);
    expect(svc.checkWorkPermitApproval).toHaveBeenCalledWith(41);
    const refusal = await screen.findByTestId("permit-approval-refusal");
    const reason = String(lookup(enTraining, "errors.PERMIT_APPROVER_IS_SUBMITTER")).replace(
      /\{\{\w+\}\}/g,
      "",
    );
    expect(refusal).toHaveTextContent(reason.replace(/\s+/g, " ").trim());
    expect(screen.getByRole("button", { name: enPermits.transition.approve })).toBeDisabled();
    expect(svc.setWorkPermitStatus).not.toHaveBeenCalled();
  });

  it("surfaces the four-eyes refusal when the submitter tries to approve", async () => {
    svc.setWorkPermitStatus.mockRejectedValue({ code: "PERMIT_APPROVER_IS_SUBMITTER" });
    const p = permit({
      status: "pending_review",
      approved_by_id: null,
      approved_by_name: null,
      approved_at: null,
    });
    await show(p);
    const approve = await screen.findByRole("button", { name: enPermits.transition.approve });
    await waitFor(() => expect(approve).toBeEnabled());
    fireEvent.click(approve);
    await waitFor(() =>
      expect(svc.setWorkPermitStatus).toHaveBeenCalledWith({
        id: 41,
        status: "approved",
        expected_row_version: 5,
      }),
    );
    const reason = String(lookup(enTraining, "errors.PERMIT_APPROVER_IS_SUBMITTER")).replace(
      /\{\{\w+\}\}/g,
      "",
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(reason.replace(/\s+/g, " ").trim());
  });
});

// L4-15: a permit, possibly with padlocks still on, is never cancelled without a reason.
describe("PermitDetailPanel — cancelling a permit needs a reason", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 10;
    svc.listPermitIsolations.mockResolvedValue([VERIFIED_ISOLATION]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
    safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
  });

  it("asks for the reason, keeps the permit on the other button and sends the reason", async () => {
    svc.setWorkPermitStatus.mockResolvedValue(permit({ status: "cancelled" }));
    await renderPanel();
    fireEvent.click(screen.getByRole("button", { name: enPermits.transition.cancel }));
    const dialog = await screen.findByRole("dialog");
    // The two buttons say different things: keep the permit, or cancel it.
    expect(within(dialog).getByRole("button", { name: enPermits.transition.keep })).toBeEnabled();
    const submit = within(dialog).getByRole("button", { name: enPermits.transition.cancel });
    expect(submit).toBeDisabled();
    const reason = within(dialog).getByLabelText(enPermits.transition.cancelReason, {
      exact: false,
    });
    fireEvent.change(reason, { target: { value: "   " } });
    expect(submit).toBeDisabled();
    fireEvent.change(reason, { target: { value: "  Work postponed: part not delivered " } });
    expect(submit).toBeEnabled();
    fireEvent.click(submit);
    await waitFor(() => expect(svc.setWorkPermitStatus).toHaveBeenCalledTimes(1));
    expect(svc.setWorkPermitStatus).toHaveBeenCalledWith(
      expect.objectContaining({
        status: "cancelled",
        reason: "Work postponed: part not delivered",
      }),
    );
  });

  it("records nothing when the person keeps the permit", async () => {
    await renderPanel();
    fireEvent.click(screen.getByRole("button", { name: enPermits.transition.cancel }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: enPermits.transition.keep }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(svc.setWorkPermitStatus).not.toHaveBeenCalled();
  });

  it("shows who cancelled the permit, when and why", async () => {
    await renderPanel(
      permit({
        status: "cancelled",
        cancel_reason: "Work postponed",
        cancelled_by_id: 8,
        cancelled_by_name: "Sara Sup",
        cancelled_at: "2026-11-04T10:00:00Z",
      }),
    );
    const row = screen.getByTestId("permit-cancelled");
    expect(row).toHaveTextContent("Sara Sup");
    expect(row).toHaveTextContent(
      fill(enPermits.detail.cancelReason, { reason: "Work postponed" }),
    );
  });

  it("says the reason was not recorded on a permit cancelled before it was asked", async () => {
    await renderPanel(permit({ status: "cancelled" }));
    const row = screen.getByTestId("permit-cancelled");
    expect(row).toHaveTextContent(enPermits.detail.cancelNotRecorded);
    expect(row).toHaveTextContent(enPermits.detail.cancelReasonNotRecorded);
  });
});

describe("PermitDetailPanel — verifying an isolation with the zero-energy test", () => {
  const applied: PermitIsolationRecord = {
    ...LOCK_ON,
    verified_by_id: null,
    verified_at: null,
    verified_by_name: null,
    row_version: 2,
  };
  const issued = permit({ status: "issued", activated_at: null, row_version: 4 });

  beforeEach(() => {
    vi.clearAllMocks();
    siteRights.withheld = new Set();
    session.userId = 8;
    svc.listPermitIsolations.mockResolvedValue([applied]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
    svc.listPermitHandoverRoles.mockResolvedValue([]);
    svc.listPermitHandoverReceivers.mockResolvedValue({ items: [], total: 0 });
    safetySvc.getPermitSafetyContent.mockResolvedValue(SAFETY);
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([]);
  });

  async function openVerify() {
    const row = await renderPanel(issued);
    fireEvent.click(within(row).getByRole("button", { name: enPermits.isolation.verify }));
    const dialog = await screen.findByRole("dialog");
    return {
      dialog,
      confirm: within(dialog).getByRole("button", { name: enPermits.isolation.verify }),
      box: within(dialog).getByRole("checkbox"),
      note: within(dialog).getByRole("textbox"),
    };
  }

  it("keeps the confirmation disabled until the test is ticked and commented", async () => {
    const { dialog, confirm, box, note } = await openVerify();
    expect(dialog).toHaveTextContent(enPermits.isolation.zeroEnergyConfirm);
    expect(confirm).toBeDisabled();
    // A comment alone is not enough…
    fireEvent.change(note, { target: { value: "VAT 0 V" } });
    expect(confirm).toBeDisabled();
    // …nor the box with a blank comment.
    fireEvent.click(box);
    fireEvent.change(note, { target: { value: "   " } });
    expect(confirm).toBeDisabled();
    fireEvent.change(note, { target: { value: "VAT 0 V" } });
    expect(confirm).toBeEnabled();
    expect(svc.verifyPermitIsolation).not.toHaveBeenCalled();
  });

  it("sends the confirmation and the trimmed comment", async () => {
    svc.verifyPermitIsolation.mockResolvedValue({
      ...applied,
      verified_at: "2026-10-10T09:10:00Z",
    });
    const { confirm, box, note } = await openVerify();
    fireEvent.click(box);
    fireEvent.change(note, { target: { value: "  Essai de démarrage refusé, VAT 0 V " } });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(svc.verifyPermitIsolation).toHaveBeenCalledWith({
        isolation_id: 300,
        expected_row_version: 2,
        lock_number: null,
        zero_energy_confirmed: true,
        zero_energy_note: "Essai de démarrage refusé, VAT 0 V",
      }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows the recorded test under the verifier", async () => {
    svc.listPermitIsolations.mockResolvedValue([
      { ...LOCK_ON, zero_energy_note: "Essai de démarrage refusé, VAT 0 V" },
    ]);
    const row = await renderPanel();
    expect(within(row).getByTestId("isolation-zero-energy")).toHaveTextContent(
      fill(enPermits.isolation.zeroEnergyShown, { note: "Essai de démarrage refusé, VAT 0 V" }),
    );
  });

  it("says the test was not recorded on a line verified before it existed", async () => {
    svc.listPermitIsolations.mockResolvedValue([LOCK_ON]);
    const row = await renderPanel();
    expect(within(row).getByTestId("isolation-zero-energy")).toHaveTextContent(
      enPermits.isolation.zeroEnergyNotRecorded,
    );
  });
});

function renderApproved() {
  render(
    <MemoryRouter>
      <PermitDetailPanel
        permit={permit({ status: "approved", issued_at: null, activated_at: null })}
        onChanged={vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe("PermitDetailPanel issue gate and proposed isolation points (L4-03 / L4-04 / L4-12)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    session.userId = 10;
    siteRights.withheld = new Set();
    svc.listPermitIsolations.mockResolvedValue([]);
    svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
    svc.listPermitSuspensions.mockResolvedValue([]);
    svc.listPermitHandoverLogs.mockResolvedValue([]);
  });

  it("greys the issue while the server reports blockers, and lists them", async () => {
    safetySvc.getPermitSafetyContent.mockResolvedValue({
      ...SAFETY,
      permit_status: "approved",
      ppe_required: null,
      requires_gas_test: true,
      issue_blockers: [
        { code: "GATE_PERMIT_PPE_MISSING", params: { permit: "PTW-2026-000041" } },
        { code: "GATE_PERMIT_GAS_TEST_MISSING", params: { permit: "PTW-2026-000041" } },
      ],
    });
    renderApproved();
    const issue = await screen.findByRole("button", { name: enPermits.transition.issue });
    await waitFor(() => expect(issue).toBeDisabled());
    expect(issue).toHaveAttribute("title", enPermits.safety.issueBlocked);
    const blockers = await screen.findByTestId("permit-issue-blockers");
    expect(blockers).toHaveTextContent("gateError.GATE_PERMIT_PPE_MISSING");
    expect(blockers).toHaveTextContent("gateError.GATE_PERMIT_GAS_TEST_MISSING");
    expect(screen.getByTestId("permit-gas-limits")).toHaveTextContent(/19[.,]5 .*LEL < 10 %/);
  });

  it("offers the issue when nothing blocks it", async () => {
    safetySvc.getPermitSafetyContent.mockResolvedValue({ ...SAFETY, permit_status: "approved" });
    renderApproved();
    const issue = await screen.findByRole("button", { name: enPermits.transition.issue });
    await waitFor(() => expect(issue).toBeEnabled());
    expect(screen.queryByTestId("permit-issue-blockers")).toBeNull();
  });

  it("pre-fills the isolation form from a point of the equipment, still editable", async () => {
    safetySvc.listPermitProposedIsolationPoints.mockResolvedValue([
      {
        point: {
          id: 5,
          entity_sync_id: "ip5",
          equipment_id: 900,
          point_code: "IP-01",
          label: "Main breaker MCC-3",
          energy_type_id: 11,
          energy_type_code: "ELECTRICAL",
          energy_type_label: "Électrique",
          isolation_method_id: 12,
          isolation_method_label: "Disjoncteur cadenassé",
          normal_state: "Closed",
          is_active: true,
          row_version: 1,
        },
        isolation_point_text: "IP-01 — Main breaker MCC-3",
        already_on_permit: false,
      },
    ]);
    renderApproved();
    const box = await screen.findByTestId("permit-proposed-points");
    fireEvent.click(within(box).getByRole("button", { name: enPermits.safety.proposed.use }));
    const point = document.getElementById("iso-point") as HTMLInputElement;
    expect(point.value).toBe("IP-01 — Main breaker MCC-3");
    fireEvent.change(point, { target: { value: "IP-01 — Main breaker MCC-3 (side B)" } });
    expect(point.value).toBe("IP-01 — Main breaker MCC-3 (side B)");
  });
});

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import type { WorkPermitRecord } from "@/services/permit-service";
import type {
  PermitPersonalLock,
  PermitWorker,
  PermitWorkersView,
} from "@/services/permit-workers-service";

import { PermitWorkersPanel } from "../PermitWorkersPanel";

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

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (ns !== "permits") return key;
      const value = lookup(enPermits, key);
      if (value == null) throw new Error(`missing permits key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  listPermitWorkers: vi.fn(),
  addPermitWorker: vi.fn(),
  recordPermitWorkerInduction: vi.fn(),
  withdrawPermitWorker: vi.fn(),
  placeMyPermitLock: vi.fn(),
  removeMyPermitLock: vi.fn(),
  forceRemovePermitLock: vi.fn(),
  listPermitPadlockRemovers: vi.fn(),
  acknowledgeForcedPadlockRemoval: vi.fn(),
  recordPadlockWorkerInformed: vi.fn(),
}));
vi.mock("@/services/permit-service", () => ({
  listPermitContactMethods: vi.fn().mockResolvedValue([]),
  forcePermitIsolationRemoval: vi.fn(),
  listPermitLockRemovers: vi.fn(),
}));
vi.mock("@/services/permit-workers-service", () => svc);
vi.mock("@/services/personnel-service", () => ({
  listPersonnel: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  listExternalCompanies: vi.fn().mockResolvedValue([]),
}));

const PERMIT = {
  id: 41,
  code: "PTW-2026-000041",
  status: "active",
  row_version: 5,
} as unknown as WorkPermitRecord;

function worker(overrides: Partial<PermitWorker> = {}): PermitWorker {
  return {
    id: 7,
    entity_sync_id: "w7",
    permit_id: 41,
    personnel_id: 3,
    personnel_name: "Omar Mechanic",
    employee_code: "W-L",
    external_name: null,
    external_company_id: null,
    company_name: null,
    is_external: false,
    role_on_permit: "Mechanic",
    induction_at: null,
    induction_by_id: null,
    induction_by_name: null,
    added_by_id: 1,
    added_by_name: "Admin",
    added_at: "2026-11-11T08:00:00Z",
    withdrawn_at: null,
    row_version: 1,
    is_me: true,
    open_lock: null,
    blockers: [],
    ...overrides,
  };
}

function view(workers: PermitWorker[]): PermitWorkersView {
  return { permit_id: 41, permit_status: "active", workers, locks: [], open_lock_count: 0 };
}

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("PermitWorkersPanel", () => {
  it("lets the listed worker place only their own padlock, with the narrow right", async () => {
    svc.listPermitWorkers.mockResolvedValue(view([worker()]));
    svc.placeMyPermitLock.mockResolvedValue(view([worker()]));
    const onChanged = vi.fn();
    render(
      <PermitWorkersPanel
        permit={PERMIT}
        canIssue={false}
        canClose={false}
        canLockOwn
        onChanged={onChanged}
      />,
    );
    await screen.findByText("Omar Mechanic");
    // No broader right: no add form, no withdraw.
    expect(screen.queryByTestId("permit-worker-add")).toBeNull();
    expect(screen.queryByText(enPermits.workers.withdraw)).toBeNull();
    fireEvent.change(screen.getByLabelText(new RegExp(enPermits.workers.myLock.number)), {
      target: { value: " C-12 " },
    });
    fireEvent.click(screen.getByRole("button", { name: enPermits.workers.myLock.place }));
    await waitFor(() =>
      expect(svc.placeMyPermitLock).toHaveBeenCalledWith({ permit_id: 41, lock_number: "C-12" }),
    );
    expect(onChanged).toHaveBeenCalled();
  });

  it("offers no padlock without the padlock right, and shows the server's blockers", async () => {
    svc.listPermitWorkers.mockResolvedValue(
      view([
        worker({
          is_me: false,
          is_external: true,
          personnel_id: null,
          personnel_name: null,
          external_name: "Yacine",
          company_name: "Weld Co",
          blockers: [
            { code: "GATE_PERMIT_WORKER_INDUCTION_MISSING", params: { worker: "Yacine" } },
          ],
        }),
      ]),
    );
    render(
      <PermitWorkersPanel
        permit={PERMIT}
        canIssue
        canClose={false}
        canLockOwn={false}
        onChanged={vi.fn()}
      />,
    );
    await screen.findByText("Yacine");
    expect(screen.getByTestId("permit-worker-blockers")).toBeTruthy();
    expect(screen.queryByRole("button", { name: enPermits.workers.myLock.place })).toBeNull();
    expect(screen.getByRole("button", { name: enPermits.workers.recordInduction })).toBeTruthy();
  });

  it("asks the owner of a padlock removed by force to acknowledge it before placing again", async () => {
    const forced: PermitPersonalLock = {
      id: 9,
      entity_sync_id: "l9",
      permit_id: 41,
      worker_id: 7,
      lock_number: "C-12",
      placed_by_id: 2,
      placed_by_name: "Omar Mechanic",
      placed_at: "2026-11-11T08:00:00Z",
      removed_by_id: 5,
      removed_by_name: "Remover",
      removed_at: "2026-11-11T18:00:00Z",
      forced_removal: true,
      forced_reason: "Left the site without removing the padlock",
      forced_authorized_by_id: 4,
      forced_authorized_by_name: "Shift lead",
      forced_worker_absent: true,
      forced_contact_attempts: [
        { at: "2026-11-11T17:50:00Z", method: "PHONE", result: "No answer" },
      ],
      informed_by_id: null,
      informed_by_name: null,
      informed_at: null,
      informed_method: null,
      informed_in_app: false,
      row_version: 2,
    };
    svc.listPermitWorkers.mockResolvedValue({ ...view([worker()]), locks: [forced] });
    svc.acknowledgeForcedPadlockRemoval.mockResolvedValue(view([worker()]));
    render(
      <PermitWorkersPanel
        permit={PERMIT}
        canIssue={false}
        canClose={false}
        canLockOwn
        onChanged={vi.fn()}
      />,
    );
    await screen.findByTestId("permit-lock-untold");
    fireEvent.click(screen.getByRole("button", { name: enPermits.workers.told.acknowledge }));
    await waitFor(() => expect(svc.acknowledgeForcedPadlockRemoval).toHaveBeenCalledWith(9));
  });
});

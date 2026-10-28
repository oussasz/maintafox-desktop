/**
 * WoCloseOutPanel — closing a work order is confirmed in a dialog that names the OT
 * (owner decision 2026-10-14: no password re-entry; RBAC and audit stay on the backend).
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enOt from "@/i18n/locale-data/en/ot.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import { CloseoutBlockingError } from "@/services/wo-closeout-service";
import type * as CloseoutService from "@/services/wo-closeout-service";
import type * as WoService from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

import { WoCloseOutPanel } from "../WoCloseOutPanel";

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

const CATALOGS: Record<string, unknown> = { ot: enOt, permits: enPermits, common: enCommon };

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = lookup(CATALOGS[ns ?? "ot"], key);
      if (value == null)
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const closeout = vi.hoisted(() => ({
  saveFailureDetail: vi.fn(),
  saveVerification: vi.fn(),
  closeWo: vi.fn(),
  reopenWo: vi.fn(),
  updateWoRca: vi.fn(),
  updateServiceCost: vi.fn(),
  getWoAnalyticsSnapshot: vi.fn(),
  getWoForCloseOut: vi.fn(),
}));
vi.mock("@/services/wo-closeout-service", async (importOriginal) => {
  const actual = await importOriginal<typeof CloseoutService>();
  return { ...actual, ...closeout };
});
vi.mock("@/services/wo-service", async (importOriginal) => ({
  ...(await importOriginal<typeof WoService>()),
  evaluateWoCompletionGates: vi.fn(async () => []),
}));
vi.mock("@/services/settings-service", () => ({ getSetting: vi.fn(async () => null) }));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/common/LinkedEntityBadge", () => ({ LinkedEntityBadge: () => null }));
vi.mock("../use-wo-people", () => ({
  useWoPeopleOptions: () => ({ options: [], loading: false, failed: false }),
  mergeWoPersonOptions: (base: unknown[]) => base,
}));
vi.mock("@/hooks/use-permissions", () => ({ usePermissions: () => ({ can: () => true }) }));
// Rights at the work order’s site (`useWoAccess`): granted here.
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: () => true }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 5 } }) }));
const stepUp = vi.hoisted(() => ({ withStepUp: vi.fn() }));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: stepUp.withStepUp, StepUpDialogElement: null }),
}));
vi.mock("@/stores/wo-store", () => ({
  useWoStore: (sel: (s: { refreshActiveWo: () => Promise<void> }) => unknown) =>
    sel({ refreshActiveWo: vi.fn(async () => {}) }),
}));

const WO = {
  id: 42,
  code: "OT-0042",
  status_code: "completed",
  row_version: 7,
  production_impact_id: null,
} as unknown as WorkOrder;

describe("WoCloseOutPanel close confirmation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    closeout.getWoAnalyticsSnapshot.mockResolvedValue(null);
    closeout.getWoForCloseOut.mockResolvedValue({ wo: WO, failure_detail: null });
  });

  it("closes only after the user confirms a summary naming the work order, without a password", async () => {
    const onClosed = vi.fn();
    closeout.closeWo.mockResolvedValue({ ...WO, status_code: "closed" });
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={onClosed} />);

    fireEvent.click(screen.getByRole("button", { name: enOt.closeout.closeWo }));
    const confirm = await screen.findByRole("dialog", { name: enOt.closeout.confirmCloseTitle });
    expect(confirm).toHaveTextContent("OT-0042 will be closed and become read-only");
    expect(closeout.closeWo).not.toHaveBeenCalled();

    fireEvent.click(
      within(confirm).getByRole("button", { name: enOt.closeout.confirmCloseAction }),
    );
    await waitFor(() => expect(onClosed).toHaveBeenCalledTimes(1));
    expect(closeout.closeWo).toHaveBeenCalledTimes(1);
    expect(closeout.closeWo).toHaveBeenCalledWith(
      expect.objectContaining({ wo_id: 42, actor_id: 5, expected_row_version: 7 }),
    );
    expect(stepUp.withStepUp).not.toHaveBeenCalled();
  });

  it("keeps the close-out blocking reasons visible when the close is refused", async () => {
    closeout.closeWo.mockRejectedValue(
      new CloseoutBlockingError("blocked", ["Failure coding is incomplete"]),
    );
    const onClosed = vi.fn();
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={onClosed} />);

    fireEvent.click(screen.getByRole("button", { name: enOt.closeout.closeWo }));
    const confirm = await screen.findByRole("dialog", { name: enOt.closeout.confirmCloseTitle });
    fireEvent.click(
      within(confirm).getByRole("button", { name: enOt.closeout.confirmCloseAction }),
    );

    expect(await screen.findByText("Failure coding is incomplete")).toBeInTheDocument();
    expect(onClosed).not.toHaveBeenCalled();
  });

  const CONFLICT = {
    code: "VALIDATION_FAILED",
    message:
      "Validation failed: GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : cet enregistrement a ete modifie par un autre utilisateur.",
  };
  const CODING = {
    id: 1,
    work_order_id: 42,
    symptom_id: null,
    failure_mode_id: 501,
    failure_cause_id: 601,
    failure_effect_id: null,
    is_temporary_repair: false,
    is_permanent_repair: true,
    cause_not_determined: false,
    notes: "Roulement grippé",
  };
  const narrativeField = () =>
    screen.getByLabelText(enOt.closeout.narrative) as HTMLTextAreaElement;
  const save = () =>
    fireEvent.click(screen.getByRole("button", { name: enOt.closeout.saveDetails }));

  it("shows the stored failure coding of an open work order, without the analytics snapshot (AU-18)", async () => {
    const open = { ...WO, status_code: "in_progress" } as WorkOrder;
    closeout.getWoAnalyticsSnapshot.mockRejectedValue({
      code: "VALIDATION_FAILED",
      message: "Validation failed: GATE_WO_ANALYTICS_STATUS:status=in_progress: refused",
    });
    closeout.getWoForCloseOut.mockResolvedValue({ wo: open, failure_detail: CODING });
    render(<WoCloseOutPanel wo={open} canEdit onClosed={vi.fn()} />);

    await waitFor(() => expect(narrativeField().value).toBe("Roulement grippé"));
    closeout.saveFailureDetail.mockResolvedValue({ ...CODING, wo_row_version: 8 });
    closeout.updateWoRca.mockResolvedValue(9);
    save();
    await waitFor(() => expect(closeout.updateWoRca).toHaveBeenCalledTimes(1));
    // The stored coding is saved back as it was loaded, never blanked.
    expect(closeout.saveFailureDetail).toHaveBeenCalledWith(
      expect.objectContaining({
        expected_row_version: 7,
        failure_mode_id: 501,
        failure_cause_id: 601,
        notes: "Roulement grippé",
      }),
    );
    expect(screen.queryByText(/refused/)).not.toBeInTheDocument();
  });

  it("saves the close-out coding at the version it loaded, chaining each save's new version (AU-18)", async () => {
    closeout.saveFailureDetail.mockResolvedValue({ id: 1, work_order_id: 42, wo_row_version: 8 });
    closeout.updateWoRca.mockResolvedValue(9);
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={vi.fn()} />);
    await waitFor(() => expect(closeout.getWoForCloseOut).toHaveBeenCalledWith(42));

    save();
    await waitFor(() => expect(closeout.updateWoRca).toHaveBeenCalledTimes(1));
    expect(closeout.saveFailureDetail).toHaveBeenCalledWith(
      expect.objectContaining({ wo_id: 42, expected_row_version: 7 }),
    );
    expect(closeout.updateWoRca).toHaveBeenCalledWith(
      expect.objectContaining({ wo_id: 42, expected_row_version: 8 }),
    );

    // The next save starts from the version the last one left.
    save();
    await waitFor(() => expect(closeout.saveFailureDetail).toHaveBeenCalledTimes(2));
    expect(closeout.saveFailureDetail).toHaveBeenLastCalledWith(
      expect.objectContaining({ expected_row_version: 9 }),
    );
  });

  it("keeps the version of a step that succeeded when a later step fails (AU-18)", async () => {
    closeout.saveFailureDetail.mockResolvedValue({ id: 1, work_order_id: 42, wo_row_version: 8 });
    closeout.updateWoRca.mockRejectedValueOnce({ code: "INTERNAL", message: "disk full" });
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={vi.fn()} />);
    await waitFor(() => expect(closeout.getWoForCloseOut).toHaveBeenCalledTimes(1));

    save();
    await waitFor(() => expect(closeout.updateWoRca).toHaveBeenCalledTimes(1));
    closeout.updateWoRca.mockResolvedValue(10);
    closeout.saveFailureDetail.mockResolvedValue({ id: 1, work_order_id: 42, wo_row_version: 9 });
    save();
    await waitFor(() => expect(closeout.saveFailureDetail).toHaveBeenCalledTimes(2));
    // Saved at the version the first step left, not refused as "changed by someone else".
    expect(closeout.saveFailureDetail).toHaveBeenLastCalledWith(
      expect.objectContaining({ expected_row_version: 8 }),
    );
  });

  it("shows a close-out change made elsewhere instead of saving over it (AU-18)", async () => {
    closeout.getWoForCloseOut.mockResolvedValueOnce({ wo: WO, failure_detail: CODING });
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={vi.fn()} />);
    await waitFor(() => expect(narrativeField().value).toBe("Roulement grippé"));
    fireEvent.change(narrativeField(), { target: { value: "Mine" } });

    // Someone else changed the coding: the save is refused and their values replace the screen.
    closeout.saveFailureDetail.mockRejectedValueOnce(CONFLICT);
    closeout.getWoForCloseOut.mockResolvedValue({
      wo: { ...WO, row_version: 8 },
      failure_detail: { ...CODING, failure_mode_id: 777, notes: "Theirs" },
    });
    save();
    await waitFor(() => expect(narrativeField().value).toBe("Theirs"));
    expect(closeout.saveFailureDetail).toHaveBeenCalledTimes(1);
    expect(closeout.updateWoRca).not.toHaveBeenCalled();

    // The next save is made on their version, with the values now on screen.
    closeout.saveFailureDetail.mockResolvedValue({ ...CODING, wo_row_version: 9 });
    closeout.updateWoRca.mockResolvedValue(10);
    save();
    await waitFor(() => expect(closeout.saveFailureDetail).toHaveBeenCalledTimes(2));
    expect(closeout.saveFailureDetail).toHaveBeenLastCalledWith(
      expect.objectContaining({ expected_row_version: 8, failure_mode_id: 777, notes: "Theirs" }),
    );
  });

  it("saves what was typed when only another field of the work order moved (AU-18)", async () => {
    closeout.getWoForCloseOut.mockResolvedValueOnce({ wo: WO, failure_detail: CODING });
    render(<WoCloseOutPanel wo={WO} canEdit onClosed={vi.fn()} />);
    await waitFor(() => expect(narrativeField().value).toBe("Roulement grippé"));
    fireEvent.change(narrativeField(), { target: { value: "Mine" } });

    // A header action (e.g. Terminer) moved the version; the close-out is as it was loaded.
    closeout.saveFailureDetail
      .mockRejectedValueOnce(CONFLICT)
      .mockResolvedValue({ ...CODING, wo_row_version: 9 });
    closeout.getWoForCloseOut.mockResolvedValue({
      wo: { ...WO, row_version: 8 },
      failure_detail: CODING,
    });
    closeout.updateWoRca.mockResolvedValue(10);
    save();
    await waitFor(() => expect(closeout.updateWoRca).toHaveBeenCalledTimes(1));
    expect(closeout.saveFailureDetail).toHaveBeenLastCalledWith(
      expect.objectContaining({ expected_row_version: 8, notes: "Mine" }),
    );
    expect(narrativeField().value).toBe("Mine");
  });
});

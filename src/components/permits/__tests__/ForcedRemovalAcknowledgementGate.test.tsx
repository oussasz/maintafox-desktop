import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enPermits from "@/i18n/locale-data/en/permits.json";
import enTraining from "@/i18n/locale-data/en/training.json";
import type { ForcedRemovalItem, PermitIsolationRecord } from "@/services/permit-service";

import { ForcedRemovalAcknowledgementGate } from "../ForcedRemovalAcknowledgementGate";

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
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = key.startsWith("permits:") ? lookup(enPermits, key.slice(8)) : undefined;
      if (value == null) throw new Error(`missing key: ${key}`);
      return fill(value, opts);
    },
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));

vi.mock("@/lib/app-error", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  appErrorReason: (e: { code?: string }) =>
    ((e.code && lookup(enTraining, `errors.${e.code}`)) || "unknown").replace(/\{\{\w+\}\}/g, ""),
}));

const svc = vi.hoisted(() => ({
  listMyForcedLockRemovals: vi.fn(),
  acknowledgeForcedLockRemoval: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

const isolation = (id: number): PermitIsolationRecord => ({
  id,
  entity_sync_id: `i${id}`,
  permit_id: 41,
  isolation_point: `MCC-${id} breaker`,
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
  row_version: 4,
  applied_by_name: "Amine Tech",
  verified_by_name: "Sara Sup",
  removal_verified_by_name: null,
  cancelled_at: null,
  cancelled_by_id: null,
  cancelled_by_name: null,
  removed_by_id: 12,
  removed_by_name: "Rémi Remover",
  removed_at: "2026-10-10T17:00:00Z",
  forced_removal: true,
  forced_authorized_by_id: 11,
  forced_authorized_by_name: "Chef Site",
  forced_reason: "Fin de poste, travailleur parti",
  forced_worker_absent: true,
  forced_contact_attempts: [
    { at: "2026-10-10T16:30:00Z", method: "Téléphone", result: "Pas de réponse" },
  ],
  informed_by_id: null,
  informed_by_name: null,
  informed_at: null,
  informed_method: null,
  informed_in_app: false,
});

const item = (id: number): ForcedRemovalItem => ({
  isolation: isolation(id),
  permit_id: 41,
  permit_code: "PTW-2026-000041",
  permit_status: "closed",
  equipment_label: "PMP-101 — Feed pump",
});

describe("ForcedRemovalAcknowledgementGate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("blocks the app at sign-in until the worker acknowledges each forced removal", async () => {
    svc.listMyForcedLockRemovals.mockResolvedValue([item(1), item(2)]);
    svc.acknowledgeForcedLockRemoval.mockResolvedValue(undefined);
    render(<ForcedRemovalAcknowledgementGate sessionUserId={7} />);
    const gate = await screen.findByRole("alertdialog");
    expect(gate).toHaveAttribute("aria-modal", "true");
    expect(gate).toHaveTextContent(enPermits.gateAck.title);
    const items = within(gate).getAllByTestId("forced-removal-gate-item");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("Chef Site");
    expect(items[0]).toHaveTextContent("Rémi Remover");
    expect(items[0]).toHaveTextContent("Fin de poste, travailleur parti");

    fireEvent.click(
      within(items[0] as HTMLElement).getByRole("button", { name: enPermits.gateAck.acknowledge }),
    );
    await waitFor(() => expect(svc.acknowledgeForcedLockRemoval).toHaveBeenCalledWith(1));
    await waitFor(() => expect(screen.getAllByTestId("forced-removal-gate-item")).toHaveLength(1));
    // Still blocking while one is left.
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: enPermits.gateAck.acknowledge }));
    await waitFor(() => expect(svc.acknowledgeForcedLockRemoval).toHaveBeenCalledWith(2));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  });

  it("keeps blocking and shows the refusal when the acknowledgement is refused", async () => {
    svc.listMyForcedLockRemovals.mockResolvedValue([item(1)]);
    svc.acknowledgeForcedLockRemoval.mockRejectedValue({ code: "PERMIT_WORKER_ACK_NOT_YOURS" });
    render(<ForcedRemovalAcknowledgementGate sessionUserId={7} />);
    fireEvent.click(await screen.findByRole("button", { name: enPermits.gateAck.acknowledge }));
    expect(
      await screen.findByText(String(lookup(enTraining, "errors.PERMIT_WORKER_ACK_NOT_YOURS"))),
    ).toBeInTheDocument();
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("shows nothing when there is nothing to acknowledge or nobody is signed in", async () => {
    svc.listMyForcedLockRemovals.mockResolvedValue([]);
    const { rerender } = render(<ForcedRemovalAcknowledgementGate sessionUserId={7} />);
    await waitFor(() => expect(svc.listMyForcedLockRemovals).toHaveBeenCalled());
    expect(screen.queryByRole("alertdialog")).toBeNull();
    svc.listMyForcedLockRemovals.mockClear();
    rerender(<ForcedRemovalAcknowledgementGate sessionUserId={null} />);
    expect(svc.listMyForcedLockRemovals).not.toHaveBeenCalled();
  });
});

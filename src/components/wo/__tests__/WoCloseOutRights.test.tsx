/**
 * WoCloseOutPanel — actions the user may not take are greyed with their reason before any click
 * (L5-08), by the same rules as the backend: `ot.edit` at the work order's site for the close-out
 * details (not on a closed or cancelled order), `ot.close` for closing a completed order.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enAdmin from "@/i18n/locale-data/en/admin.json";
import enOt from "@/i18n/locale-data/en/ot.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import type * as CloseoutService from "@/services/wo-closeout-service";
import type * as WoService from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

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

const CATALOGS: Record<string, unknown> = {
  ot: enOt,
  permits: enPermits,
  common: enCommon,
  admin: enAdmin,
};

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
/** The rights the user holds at the work order's site (`useWoAccess`). */
const rights = vi.hoisted(() => ({ held: new Set<string>(), loading: false, failed: false }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => rights.held.has(p) }),
}));
// As the real hook: while loading, or after a failed read, `can` answers false for everything.
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({
    loading: rights.loading,
    failed: rights.failed,
    can: (p: string) => !rights.loading && !rights.failed && rights.held.has(p),
  }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 5 } }) }));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: vi.fn(), StepUpDialogElement: null }),
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

const saveButton = () => screen.getByRole("button", { name: enOt.closeout.saveDetails });
const closeButton = () => screen.getByRole("button", { name: enOt.closeout.closeWo });
const rightText = (label: string) =>
  enOt.closeout.blocked.rightRequired.replace("{{right}}", label);

function show(wo: WorkOrder, canEdit: boolean) {
  closeout.getWoForCloseOut.mockResolvedValue({ wo, failure_detail: null });
  render(<WoCloseOutPanel wo={wo} canEdit={canEdit} onClosed={vi.fn()} />);
}

describe("WoCloseOutPanel actions by right and state (L5-08)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    rights.held = new Set([P.OT_VIEW, P.OT_EDIT, P.OT_CLOSE]);
    rights.loading = false;
    rights.failed = false;
    closeout.getWoAnalyticsSnapshot.mockResolvedValue(null);
  });

  it("says no right is missing while the rights are still loading", async () => {
    rights.loading = true;
    // The dialog's edit right comes from the same loading answer: false meanwhile.
    show(WO, false);
    await waitFor(() => expect(closeout.getWoForCloseOut).toHaveBeenCalledWith(42));
    expect(saveButton()).toBeDisabled();
    expect(saveButton()).not.toHaveAttribute("title");
    expect(screen.queryByText(/Right required/)).toBeNull();
    expect(screen.queryByTestId("wo-close-blocked")).toBeNull();
  });

  it("says no right is missing when the rights cannot be read (the server decides)", async () => {
    rights.failed = true;
    show(WO, false);
    await waitFor(() => expect(closeout.getWoForCloseOut).toHaveBeenCalledWith(42));
    expect(saveButton()).toBeDisabled();
    expect(screen.queryByText(/Right required/)).toBeNull();
    expect(screen.queryByTestId("wo-close-blocked")).toBeNull();
  });

  it("names no missing right the panel's own read holds, whatever the dialog's read says", async () => {
    // The dialog reads the rights separately: still loading or failed there, its `canEdit` is
    // false while this panel's read holds both rights.
    show(WO, false);
    await waitFor(() => expect(closeout.getWoForCloseOut).toHaveBeenCalledWith(42));
    await waitFor(() => expect(closeButton()).toBeEnabled());
    expect(saveButton()).toBeDisabled();
    expect(saveButton()).not.toHaveAttribute("title");
    expect(screen.queryByText(/Right required/)).toBeNull();
    expect(screen.queryByTestId("wo-close-blocked")).toBeNull();
  });

  it("still says a closed work order no longer changes while the rights load", async () => {
    rights.loading = true;
    show({ ...WO, status_code: "closed" } as WorkOrder, false);
    expect(await screen.findByText(enOt.closeout.blocked.closed)).toBeVisible();
  });

  it("offers closing to a holder of ot.close without ot.edit, as close_wo does", async () => {
    rights.held = new Set([P.OT_VIEW, P.OT_CLOSE]);
    show(WO, false);
    const editReason = rightText(enAdmin.permissions.items.ot_edit.label);
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", editReason));
    expect(saveButton()).toBeDisabled();
    expect(closeButton()).toBeEnabled();
    expect(screen.getByText(enOt.closeout.closureWarning)).toBeInTheDocument();
    // The verification needs the edit right: greyed with the same reason, readable.
    const verify = screen.getByRole("button", { name: enOt.closeout.submitVerification });
    expect(verify).toBeDisabled();
    expect(verify).toHaveAttribute("aria-describedby", "wo-verify-blocker");
    expect(screen.getByText(editReason, { selector: "#wo-verify-blocker" })).toBeVisible();
  });

  it("offers saving and closing to a user holding both rights", async () => {
    show(WO, true);
    await waitFor(() => expect(saveButton()).toBeEnabled());
    expect(closeButton()).toBeEnabled();
    expect(screen.queryByTestId("wo-close-blocked")).toBeNull();
  });

  it("greys closing with the right it needs for a user without ot.close", async () => {
    rights.held = new Set([P.OT_VIEW, P.OT_EDIT]);
    show(WO, true);
    await waitFor(() => expect(saveButton()).toBeEnabled());
    const close = closeButton();
    const reason = rightText(enAdmin.permissions.items.ot_close.label);
    expect(close).toBeDisabled();
    expect(close).toHaveAttribute("title", reason);
    expect(screen.getByTestId("wo-close-blocked")).toHaveTextContent(reason);
    // The full closure form (PIN, signer, attestation) is not shown to them.
    expect(screen.queryByText(enOt.closeout.closureWarning)).toBeNull();
  });

  it("greys saving the details with the right it needs for a user without ot.edit", async () => {
    rights.held = new Set([P.OT_VIEW]);
    show(WO, false);
    const reason = rightText(enAdmin.permissions.items.ot_edit.label);
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", reason));
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText(reason, { selector: "#wo-closeout-details-blocker" })).toBeVisible();
    // Without the edit right, the closing step is not offered at all.
    expect(screen.queryByRole("button", { name: enOt.closeout.closeWo })).toBeNull();
  });

  it("says a closed work order's details no longer change", async () => {
    show({ ...WO, status_code: "closed" } as WorkOrder, true);
    const reason = enOt.closeout.blocked.closed;
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", reason));
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText(reason)).toBeVisible();
    expect(screen.queryByRole("button", { name: enOt.closeout.closeWo })).toBeNull();
  });

  it("says a cancelled work order's details no longer change", async () => {
    show({ ...WO, status_code: "cancelled" } as WorkOrder, true);
    const reason = enOt.closeout.blocked.cancelled;
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", reason));
    expect(saveButton()).toBeDisabled();
  });
});

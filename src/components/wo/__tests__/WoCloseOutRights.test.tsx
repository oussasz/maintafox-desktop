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
const rights = vi.hoisted(() => ({ held: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => rights.held.has(p) }),
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, can: (p: string) => rights.held.has(p) }),
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
    rights.held = new Set(["ot.view", "ot.edit", "ot.close"]);
    closeout.getWoAnalyticsSnapshot.mockResolvedValue(null);
  });

  it("offers saving and closing to a user holding both rights", async () => {
    show(WO, true);
    await waitFor(() => expect(saveButton()).toBeEnabled());
    expect(closeButton()).toBeEnabled();
    expect(screen.queryByTestId("wo-close-blocked")).toBeNull();
  });

  it("greys closing with the right it needs for a user without ot.close", async () => {
    rights.held = new Set(["ot.view", "ot.edit"]);
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
    rights.held = new Set(["ot.view"]);
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

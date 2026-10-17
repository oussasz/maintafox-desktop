/**
 * Purchasing rules: stored exchange rates (add, retire) and per-role approval limits.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ProcurementRulesPanel,
  isCurrencyCode,
  parseDecimal,
} from "@/components/inventory/ProcurementRulesPanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  listCurrencyExchangeRates: vi.fn(),
  createCurrencyExchangeRate: vi.fn(),
  deactivateCurrencyExchangeRate: vi.fn(),
  listProcurementApprovalLimits: vi.fn(),
  upsertProcurementApprovalLimit: vi.fn(),
}));
const rbac = vi.hoisted(() => ({ listRoles: vi.fn() }));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});
vi.mock("@/services/rbac-service", () => rbac);
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

describe("rules helpers", () => {
  it("checks ISO currency codes and decimals typed with a comma", () => {
    expect(isCurrencyCode("eur")).toBe(true);
    expect(isCurrencyCode("EURO")).toBe(false);
    expect(parseDecimal("145,25")).toBe(145.25);
    expect(Number.isNaN(parseDecimal(""))).toBe(true);
  });
});

describe("ProcurementRulesPanel", () => {
  beforeAll(async () => {
    Element.prototype.scrollIntoView = vi.fn();
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory", "common"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory, common: enCommon } },
        interpolation: { escapeValue: false },
      });
    }
  });

  beforeEach(() => {
    Object.values(svc).forEach((fn) => fn.mockReset());
    rbac.listRoles.mockReset();
    svc.listCurrencyExchangeRates.mockResolvedValue([
      {
        id: 5,
        from_currency: "EUR",
        to_currency: "USD",
        rate: 1.08,
        valid_from: "2026-10-01",
        source: "Bank",
        is_active: 1,
        created_by_name: "Amina Haddad",
        created_at: "2026-10-01T08:00:00Z",
      },
    ]);
    svc.createCurrencyExchangeRate.mockResolvedValue({});
    svc.deactivateCurrencyExchangeRate.mockResolvedValue(undefined);
    svc.listProcurementApprovalLimits.mockResolvedValue([
      {
        id: 1,
        role_id: 3,
        role_name: "Buyer",
        document_type: "REQUISITION",
        max_amount: 5000,
        is_active: 1,
        row_version: 1,
        updated_at: "2026-10-01T08:00:00Z",
      },
    ]);
    svc.upsertProcurementApprovalLimit.mockResolvedValue({});
    rbac.listRoles.mockResolvedValue([
      { id: 3, name: "Buyer", status: "active", permissions: [] },
      { id: 4, name: "Store manager", status: "active", permissions: [] },
    ]);
  });

  it("adds a stored exchange rate", async () => {
    render(<ProcurementRulesPanel />);
    expect(await screen.findByText("Bank")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add a rate" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a rate" });
    const save = within(dialog).getByRole("button", { name: "Save rate" });
    expect(save).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Supplier currency (ISO code)"), {
      target: { value: "gbp" },
    });
    fireEvent.change(within(dialog).getByLabelText(/^Value in/), { target: { value: "1,27" } });
    fireEvent.change(within(dialog).getByLabelText("Valid from"), {
      target: { value: "2026-10-14" },
    });
    await waitFor(() => expect(save).toBeEnabled());
    fireEvent.click(save);
    await waitFor(() =>
      expect(svc.createCurrencyExchangeRate).toHaveBeenCalledWith({
        from_currency: "GBP",
        to_currency: null,
        rate: 1.27,
        valid_from: "2026-10-14",
        source: null,
      }),
    );
  });

  it("shows role limits and sets an unlimited purchase-order limit", async () => {
    render(<ProcurementRulesPanel />);
    expect(await screen.findByText("Store manager")).toBeInTheDocument();
    // Requisition limits exist, so a role without one may not approve requisitions.
    expect(screen.getByText("Not allowed")).toBeInTheDocument();
    expect(screen.getAllByText("No limit set").length).toBeGreaterThan(0);

    fireEvent.click(
      screen.getByRole("button", { name: "Edit the Purchase orders limit of Store manager" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Save limit" }));
    await waitFor(() =>
      expect(svc.upsertProcurementApprovalLimit).toHaveBeenCalledWith({
        role_id: 4,
        document_type: "PURCHASE_ORDER",
        max_amount: null,
        is_active: true,
      }),
    );
  });

  it("retires a rate after confirmation", async () => {
    render(<ProcurementRulesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Retire" }));
    const confirm = await screen.findByRole("dialog", { name: "Retire this rate?" });
    fireEvent.click(within(confirm).getByRole("button", { name: "Retire" }));
    await waitFor(() => expect(svc.deactivateCurrencyExchangeRate).toHaveBeenCalledWith(5));
  });
});

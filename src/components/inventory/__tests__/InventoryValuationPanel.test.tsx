/**
 * Stock valuation report and period close: the report follows the grouping and the as-at date,
 * flags frozen figures, and a past period can be closed by a finance user.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import {
  InventoryValuationPanel,
  closePeriodBlocker,
  previousMonth,
} from "@/components/inventory/InventoryValuationPanel";
import { useFormatters } from "@/hooks/use-formatters";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  getInventoryValuationReport: vi.fn(),
  listInventoryValuationPeriods: vi.fn(),
  closeInventoryValuationPeriod: vi.fn(),
}));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

function money(value: number, currency?: string): string {
  let out = "";
  function Probe() {
    out = useFormatters().formatCurrency(value, currency);
    return null;
  }
  render(<Probe />);
  return out;
}

describe("valuation helpers", () => {
  it("proposes the previous calendar month", () => {
    expect(previousMonth(new Date(2026, 9, 14))).toEqual({
      start: "2026-09-01",
      end: "2026-09-30",
    });
    expect(previousMonth(new Date(2026, 0, 3))).toEqual({ start: "2025-12-01", end: "2025-12-31" });
  });

  it("only allows closing a complete past period", () => {
    expect(closePeriodBlocker("", "2026-09-30", "2026-10-14")).toBe("dates");
    expect(closePeriodBlocker("2026-09-30", "2026-09-01", "2026-10-14")).toBe("order");
    expect(closePeriodBlocker("2026-10-01", "2026-10-14", "2026-10-14")).toBe("future");
    expect(closePeriodBlocker("2026-09-01", "2026-09-30", "2026-10-14")).toBeNull();
  });
});

describe("InventoryValuationPanel", () => {
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
    svc.getInventoryValuationReport.mockImplementation(
      async (groupBy: string, asAt: string | null) => ({
        as_at: asAt ?? "2026-10-14",
        group_by: groupBy,
        currency_code: "DZD",
        frozen_period_code: asAt === "2026-09-30" ? "2026-09" : null,
        total_value: 1500,
        unvalued_qty: 2,
        rows: [
          {
            group_id: 2,
            group_code: "WH1",
            group_name: "Main store",
            quantity: 30,
            value: 1500,
            unvalued_qty: 2,
            article_count: 4,
          },
        ],
      }),
    );
    svc.listInventoryValuationPeriods.mockResolvedValue([
      {
        id: 1,
        period_code: "2026-08",
        period_start: "2026-08-01",
        period_end: "2026-08-31",
        status: "closed",
        currency_code: "DZD",
        total_value: 1200,
        unvalued_qty: 0,
        notes: null,
        closed_by_name: "Amina Haddad",
        closed_at: "2026-09-02T08:00:00Z",
      },
    ]);
    svc.closeInventoryValuationPeriod.mockResolvedValue({
      id: 2,
      period_code: "2026-09",
      period_start: "2026-09-01",
      period_end: "2026-09-30",
      status: "closed",
      currency_code: "DZD",
      total_value: 1500,
      unvalued_qty: 2,
      notes: null,
      closed_by_name: "Amina Haddad",
      closed_at: "2026-10-01T08:00:00Z",
    });
  });

  it("shows the value by warehouse and switches the grouping", async () => {
    render(<InventoryValuationPanel />);
    expect(await screen.findByText("Main store")).toBeInTheDocument();
    expect(svc.getInventoryValuationReport).toHaveBeenCalledWith("warehouse", null);
    expect(document.body.textContent).toContain(money(1500, "DZD"));
    expect(screen.getByText("Stock ledger")).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Group by"));
    fireEvent.click(await screen.findByRole("option", { name: "Family" }));
    await waitFor(() =>
      expect(svc.getInventoryValuationReport).toHaveBeenLastCalledWith("family", null),
    );
  });

  it("reads frozen figures at the end of a closed period (row click)", async () => {
    render(<InventoryValuationPanel />);
    fireEvent.click(await screen.findByText("Amina Haddad"));
    await waitFor(() =>
      expect(svc.getInventoryValuationReport).toHaveBeenLastCalledWith("warehouse", "2026-08-31"),
    );
  });

  it("closes the next period after the last closed one", async () => {
    render(<InventoryValuationPanel />);
    await screen.findByText("Amina Haddad");
    fireEvent.click(screen.getByRole("button", { name: "Close a period" }));
    const dialog = await screen.findByRole("dialog", { name: "Close a valuation period" });
    expect(within(dialog).getByLabelText("First day")).toHaveValue("2026-09-01");
    expect(within(dialog).getByLabelText("Last day")).toHaveValue("2026-09-30");
    fireEvent.change(within(dialog).getByLabelText("Notes"), { target: { value: "Month end" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Close period" }));
    await waitFor(() =>
      expect(svc.closeInventoryValuationPeriod).toHaveBeenCalledWith({
        period_start: "2026-09-01",
        period_end: "2026-09-30",
        notes: "Month end",
      }),
    );
    await waitFor(() => expect(svc.listInventoryValuationPeriods).toHaveBeenCalledTimes(2));
  });
});

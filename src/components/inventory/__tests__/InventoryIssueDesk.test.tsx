/**
 * Issue desk — a keyboard-wedge scan (code + Enter) finds the article, the storekeeper picks the
 * work order, and Enter in the quantity field issues through the one inventory issue command.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import {
  InventoryIssueDesk,
  expiryTone,
  fefoSuggestion,
} from "@/components/inventory/InventoryIssueDesk";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import type { LotStock } from "@/services/inventory-service";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  resolveInventoryScan: vi.fn(),
  issueInventoryStockToWorkOrder: vi.fn(),
  listInventoryArticles: vi.fn(),
  listInventoryStockBalances: vi.fn(),
  listInventoryReservations: vi.fn(),
  listInventoryLotStock: vi.fn(),
  listInventoryExpiringLots: vi.fn(),
}));
const wo = vi.hoisted(() => ({ listWos: vi.fn() }));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});
vi.mock("@/services/wo-service", () => wo);
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

function lot(partial: Partial<LotStock>): LotStock {
  return {
    lot_id: 1,
    article_id: 4,
    article_code: "ART-001",
    article_name: "Filter",
    lot_number: "L-1",
    serial_number: null,
    expiry_date: "2026-12-01",
    location_id: 3,
    location_code: "A-01",
    warehouse_id: 2,
    warehouse_code: "WH1",
    quantity: 5,
    days_to_expiry: 40,
    ...partial,
  };
}

describe("issue desk helpers", () => {
  it("suggests the first lot in stock that has not expired", () => {
    const lots = [
      lot({ lot_id: 1, days_to_expiry: -2 }),
      lot({ lot_id: 2, days_to_expiry: 5, quantity: 0 }),
      lot({ lot_id: 3, days_to_expiry: 12 }),
    ];
    expect(fefoSuggestion(lots)?.lot_id).toBe(3);
    expect(fefoSuggestion([])).toBeNull();
  });

  it("colours expiry by urgency", () => {
    expect(expiryTone(-1)).toBe("danger");
    expect(expiryTone(10)).toBe("warning");
    expect(expiryTone(90)).toBe("neutral");
    expect(expiryTone(null)).toBe("neutral");
  });
});

describe("InventoryIssueDesk", () => {
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
    wo.listWos.mockReset();
    svc.listInventoryExpiringLots.mockResolvedValue([
      lot({ lot_id: 9, lot_number: "L-NEAR", days_to_expiry: 6, expiry_date: "2026-10-20" }),
    ]);
    svc.resolveInventoryScan.mockResolvedValue({
      article_id: 4,
      article_code: "ART-001",
      article_name: "Bearing",
      unit_code: "PCS",
      lot_id: null,
      matched_on: "barcode",
    });
    svc.listInventoryStockBalances.mockResolvedValue([
      {
        article_id: 4,
        location_id: 3,
        location_code: "A-01",
        warehouse_code: "WH1",
        on_hand_qty: 10,
        reserved_qty: 0,
        available_qty: 10,
      },
    ]);
    svc.listInventoryArticles.mockResolvedValue([
      {
        id: 4,
        article_code: "ART-001",
        article_name: "Bearing",
        is_active: 1,
        requires_batch_tracking: 0,
        requires_serial_tracking: 0,
        preferred_location_id: 3,
      },
    ]);
    svc.listInventoryReservations.mockResolvedValue([]);
    svc.listInventoryLotStock.mockResolvedValue([]);
    svc.issueInventoryStockToWorkOrder.mockResolvedValue({
      reservation_id: 70,
      work_order_id: 81,
      work_order_code: "OT-2026-0144",
      lines: [
        {
          transaction_id: 900,
          lot_id: null,
          lot_number: null,
          serial_number: null,
          quantity: 2,
          unit_cost: 4,
          extended_cost: 8,
        },
      ],
      total_cost: 8,
    });
    wo.listWos.mockResolvedValue({
      items: [{ id: 81, code: "OT-2026-0144", title: "Pump P-101 bearing change" }],
      total: 1,
    });
  });

  it("scans with Enter, issues to the chosen work order with Enter and shows the cost", async () => {
    render(<InventoryIssueDesk />);
    expect(await screen.findByText(/L-NEAR/)).toBeInTheDocument();
    expect(screen.getByText("6 days left")).toBeInTheDocument();

    const scan = screen.getByLabelText("Scan or type an article");
    expect(scan).toHaveFocus();
    fireEvent.change(scan, { target: { value: "3760000000017" } });
    fireEvent.submit(scan);
    await waitFor(() => expect(svc.resolveInventoryScan).toHaveBeenCalledWith("3760000000017"));
    expect(await screen.findByText("Found by barcode")).toBeInTheDocument();

    const issue = screen.getByRole("button", { name: "Issue to work order" });
    expect(issue).toBeDisabled();
    expect(screen.getByText("Choose the work order the parts go to.")).toBeInTheDocument();

    fireEvent.click(await screen.findByRole("option", { name: /OT-2026-0144/ }));
    const qty = screen.getByLabelText("Quantity (PCS)");
    fireEvent.change(qty, { target: { value: "2" } });
    await waitFor(() => expect(issue).toBeEnabled());
    fireEvent.submit(qty);

    await waitFor(() =>
      expect(svc.issueInventoryStockToWorkOrder).toHaveBeenCalledWith({
        article_id: 4,
        location_id: 3,
        quantity: 2,
        work_order_id: 81,
        reservation_id: null,
        lot_id: null,
        notes: null,
      }),
    );
    expect(await screen.findByText("ART-001 issued to OT-2026-0144")).toBeInTheDocument();
    // Ready for the next scan.
    await waitFor(() => expect(screen.getByLabelText("Scan or type an article")).toHaveValue(""));
  });

  it("refuses more than the location holds", async () => {
    render(<InventoryIssueDesk />);
    const scan = screen.getByLabelText("Scan or type an article");
    fireEvent.change(scan, { target: { value: "ART-001" } });
    fireEvent.submit(scan);
    await screen.findByText("Found by barcode");
    fireEvent.click(await screen.findByRole("option", { name: /OT-2026-0144/ }));
    fireEvent.change(screen.getByLabelText("Quantity (PCS)"), { target: { value: "12" } });
    expect(await screen.findByText("Only 10 PCS can be issued from here.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Issue to work order" })).toBeDisabled();
  });

  it("offers the lots of a tracked article with the first-expiring one suggested", async () => {
    svc.listInventoryArticles.mockResolvedValue([
      {
        id: 4,
        article_code: "ART-001",
        article_name: "Bearing",
        is_active: 1,
        requires_batch_tracking: 1,
        requires_serial_tracking: 0,
        preferred_location_id: 3,
      },
    ]);
    svc.listInventoryLotStock.mockResolvedValue([
      lot({ lot_id: 21, lot_number: "L-A", days_to_expiry: 10 }),
      lot({ lot_id: 22, lot_number: "L-B", days_to_expiry: 50 }),
    ]);
    render(<InventoryIssueDesk />);
    const scan = screen.getByLabelText("Scan or type an article");
    fireEvent.change(scan, { target: { value: "ART-001" } });
    fireEvent.submit(scan);
    const lotField = await screen.findByLabelText("Lot / serial number");
    await waitFor(() =>
      expect(lotField).toHaveTextContent("Automatic — first expiring first (L-A)"),
    );
    fireEvent.click(lotField);
    fireEvent.click(await screen.findByRole("option", { name: /L-B/ }));
    fireEvent.click(await screen.findByRole("option", { name: /OT-2026-0144/ }));
    fireEvent.change(screen.getByLabelText("Quantity (PCS)"), { target: { value: "1" } });
    const issue = screen.getByRole("button", { name: "Issue to work order" });
    await waitFor(() => expect(issue).toBeEnabled());
    fireEvent.click(issue);
    await waitFor(() =>
      expect(svc.issueInventoryStockToWorkOrder).toHaveBeenCalledWith(
        expect.objectContaining({ lot_id: 22, quantity: 1 }),
      ),
    );
  });
});

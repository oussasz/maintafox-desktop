/**
 * Goods receipt: a PO in a supplier currency needs a rate (entered or stored for the date), and a
 * lot-tracked article is received into the lots it came with.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { ReceiveGoodsDialog, lotProblem } from "@/components/inventory/ReceiveGoodsDialog";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import type { InventoryArticleRow } from "@/services/inventory-service";
import { useLocaleStore } from "@/stores/locale-store";
import type { PurchaseOrder, StockLocation } from "@shared/ipc-types";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({
  listInventoryPurchaseOrderLines: vi.fn(),
  receiveInventoryPurchaseOrderGoods: vi.fn(),
  resolveCurrencyExchangeRate: vi.fn(),
  listInventoryArticles: vi.fn(),
}));

vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});

const po = {
  id: 601,
  po_number: "PO-1",
  status: "APPROVED",
  currency_code: "EUR",
} as unknown as PurchaseOrder & { currency_code: string | null };
const locations = [
  { id: 101, warehouse_code: "MAIN", code: "BIN", is_active: 1 },
] as unknown as StockLocation[];
const plainArticle = {
  id: 11,
  article_code: "A-11",
  requires_batch_tracking: 0,
  requires_serial_tracking: 0,
  requires_expiration: 0,
} as unknown as InventoryArticleRow;

const lot = (lotNumber: string, quantity: string, expiryDate = "") => ({
  key: lotNumber,
  lotNumber,
  serialNumber: "",
  expiryDate,
  quantity,
});

describe("lotProblem", () => {
  it("checks lots against the accepted quantity and the article rules", () => {
    const rules = { serial: false, expiry: true };
    expect(lotProblem([], 3, rules)).toBe("missing");
    expect(lotProblem([lot("L1", "3")], 3, rules)).toBe("expiry");
    expect(lotProblem([lot("L1", "2", "2027-01-01")], 3, rules)).toBe("sum");
    expect(
      lotProblem([lot("L1", "2", "2027-01-01"), lot("L2", "1", "2027-02-01")], 3, rules),
    ).toBeNull();
    expect(lotProblem([], 0, rules)).toBeNull();
    expect(
      lotProblem([{ ...lot("", "2"), serialNumber: "SN-1" }], 2, { serial: true, expiry: false }),
    ).toBe("serialQty");
  });
});

describe("ReceiveGoodsDialog", () => {
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
    useLocaleStore.setState({ activeCurrencyCode: "DZD" });
    svc.listInventoryPurchaseOrderLines.mockResolvedValue([
      {
        id: 7001,
        article_id: 11,
        article_code: "A-11",
        article_name: "Bearing",
        ordered_qty: 4,
        received_qty: 0,
        remaining_qty: 4,
        unit_price: 10,
        demand_source_ref: null,
      },
    ]);
    svc.receiveInventoryPurchaseOrderGoods.mockResolvedValue({});
    svc.resolveCurrencyExchangeRate.mockResolvedValue({
      from_currency: "EUR",
      to_currency: "DZD",
      rate: null,
      valid_from: null,
    });
  });

  async function fillLine(dialog: HTMLElement, qty: string) {
    fireEvent.click(await within(dialog).findByLabelText("Location"));
    fireEvent.click(await screen.findByRole("option", { name: "MAIN/BIN" }));
    fireEvent.change(within(dialog).getByLabelText("Received"), { target: { value: qty } });
  }

  it("asks for the rate of a foreign-currency PO when none is stored, then sends it", async () => {
    render(
      <ReceiveGoodsDialog
        open
        onOpenChange={vi.fn()}
        purchaseOrders={[po]}
        locations={locations}
        articles={[plainArticle]}
        initialPoId={601}
      />,
    );
    const dialog = await screen.findByRole("dialog");
    await fillLine(dialog, "2");
    expect(
      await within(dialog).findByText("Enter the EUR exchange rate: none is stored for this date."),
    ).toBeInTheDocument();
    const post = within(dialog).getByRole("button", { name: "Post receipt" });
    expect(post).toBeDisabled();

    fireEvent.change(within(dialog).getByLabelText("Exchange rate: 1 EUR = ? DZD"), {
      target: { value: "150" },
    });
    await waitFor(() => expect(post).toBeEnabled());
    expect(within(dialog).getByText(/Stock cost:/)).toBeInTheDocument();
    fireEvent.click(post);
    await waitFor(() =>
      expect(svc.receiveInventoryPurchaseOrderGoods).toHaveBeenCalledWith(
        expect.objectContaining({
          purchase_order_id: 601,
          exchange_rate: 150,
          posting_date: null,
          lines: [
            expect.objectContaining({
              po_line_id: 7001,
              accepted_qty: 2,
              location_id: 101,
              lots: null,
            }),
          ],
        }),
      ),
    );
  });

  it("uses the stored rate when one is valid on the receipt date", async () => {
    svc.resolveCurrencyExchangeRate.mockResolvedValue({
      from_currency: "EUR",
      to_currency: "DZD",
      rate: 148.5,
      valid_from: "2026-10-01",
    });
    render(
      <ReceiveGoodsDialog
        open
        onOpenChange={vi.fn()}
        purchaseOrders={[po]}
        locations={locations}
        articles={[plainArticle]}
        initialPoId={601}
      />,
    );
    const dialog = await screen.findByRole("dialog");
    await fillLine(dialog, "1");
    expect(await within(dialog).findByText(/stored rate 148.5/)).toBeInTheDocument();
    const post = within(dialog).getByRole("button", { name: "Post receipt" });
    await waitFor(() => expect(post).toBeEnabled());
    fireEvent.click(post);
    await waitFor(() =>
      expect(svc.receiveInventoryPurchaseOrderGoods).toHaveBeenCalledWith(
        expect.objectContaining({ exchange_rate: null }),
      ),
    );
  });

  it("records the lots of a lot-tracked article", async () => {
    render(
      <ReceiveGoodsDialog
        open
        onOpenChange={vi.fn()}
        purchaseOrders={[{ ...po, currency_code: null }]}
        locations={locations}
        articles={[{ ...plainArticle, requires_batch_tracking: 1 }]}
        initialPoId={601}
      />,
    );
    const dialog = await screen.findByRole("dialog");
    await fillLine(dialog, "3");
    const post = within(dialog).getByRole("button", { name: "Post receipt" });
    expect(await within(dialog).findByText(/enter the lot or serial number/)).toBeInTheDocument();
    fireEvent.change(within(dialog).getByLabelText("Lot number"), { target: { value: "L-77" } });
    await waitFor(() => expect(post).toBeEnabled());
    fireEvent.click(post);
    await waitFor(() =>
      expect(svc.receiveInventoryPurchaseOrderGoods).toHaveBeenCalledWith(
        expect.objectContaining({
          exchange_rate: null,
          lines: [
            expect.objectContaining({
              lots: [{ lot_number: "L-77", serial_number: null, expiry_date: null, quantity: 3 }],
            }),
          ],
        }),
      ),
    );
  });
});

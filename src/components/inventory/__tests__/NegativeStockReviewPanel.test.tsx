import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import type { ReactNode } from "react";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { NegativeStockReviewPanel } from "@/components/inventory/NegativeStockReviewPanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import frInventory from "@/i18n/locale-data/fr/inventory.json";
import type * as NegativeStockService from "@/services/negative-stock-service";

const mocks = vi.hoisted(() => ({
  listNegativeStockReviews: vi.fn(),
  resolveNegativeStockByCount: vi.fn(),
  getNegativeStockSummary: vi.fn(),
}));
const lookupMocks = vi.hoisted(() => ({ getLookupValues: vi.fn() }));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("@/services/negative-stock-service", async (importOriginal) => ({
  ...(await importOriginal<typeof NegativeStockService>()),
  ...mocks,
}));
vi.mock("@/services/lookup-service", () => lookupMocks);

const TS = "2026-10-20T08:00:00Z";

function item(overrides: Record<string, unknown> = {}) {
  return {
    id: 1,
    article_id: 21,
    article_code: "A-21",
    article_name: "Bearing",
    location_id: 10,
    location_code: "BIN",
    warehouse_code: "MAIN",
    status: "open",
    opened_at: TS,
    opening_transaction_id: 99,
    opening_movement_type: "ISSUE",
    closed_at: null,
    closure_kind: null,
    closing_transaction_id: null,
    closing_movement_type: null,
    on_hand_qty: -5,
    reserved_qty: 0,
    available_qty: -5,
    ...overrides,
  };
}

describe("NegativeStockReviewPanel", () => {
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
    } else {
      i18next.addResourceBundle("en", "inventory", enInventory, true, true);
      i18next.addResourceBundle("en", "common", enCommon, true, true);
    }
  });

  beforeEach(() => {
    Object.values(mocks).forEach((fn) => fn.mockReset());
    lookupMocks.getLookupValues.mockReset();
    lookupMocks.getLookupValues.mockResolvedValue([
      {
        id: 1,
        code: "COUNT_ERROR",
        label: "Erreur de comptage",
        fr_label: "Erreur de comptage",
        en_label: "Count error",
        color: null,
        is_active: 1,
      },
    ]);
  });

  it("flags a location below zero once, and shows over-reservations as a warning only", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [item()],
      double_counts: [],
      over_reservations: [
        {
          article_id: 22,
          article_code: "A-22",
          article_name: "Seal",
          location_id: 11,
          location_code: "BIN2",
          warehouse_code: "MAIN",
          on_hand_qty: 5,
          reserved_qty: 7,
          available_qty: -2,
        },
      ],
    });
    render(<NegativeStockReviewPanel />);

    expect(await screen.findByText("Negative — to reconcile")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Record a count" })).toHaveLength(1);
    expect(screen.getByText("Reservations above stock (warning)")).toBeInTheDocument();
    expect(screen.getByText(/A-22 — Seal at MAIN\/BIN2/)).toBeInTheDocument();
    // Nothing to reject or accept as is: the count is the one action.
    expect(screen.queryByRole("button", { name: /reject|accept/i })).not.toBeInTheDocument();
    expect(mocks.listNegativeStockReviews).toHaveBeenCalledWith(false);
  });

  it("records a count only with a counted quantity, a variance reason and an 8-character reason", async () => {
    mocks.listNegativeStockReviews.mockResolvedValueOnce({
      items: [item()],
      double_counts: [],
      over_reservations: [],
    });
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [],
      over_reservations: [],
    });
    mocks.resolveNegativeStockByCount.mockResolvedValue({
      session_code: "CC-1",
      counted_qty: 0,
      variance_qty: 5,
      review: item({ status: "closed", closure_kind: "count", closed_at: TS, on_hand_qty: 0 }),
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Record a count" }));

    const confirm = screen.getByRole("button", { name: "Record the count" });
    fireEvent.change(screen.getByLabelText("Quantity counted"), { target: { value: "0" } });
    fireEvent.click(screen.getByLabelText("Variance reason"));
    fireEvent.click(await screen.findByRole("option", { name: "Count error" }));
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: "  abcdefg  " } });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason"), { target: { value: " Bin empty " } });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(mocks.resolveNegativeStockByCount).toHaveBeenCalledWith({
        article_id: 21,
        location_id: 10,
        counted_qty: 0,
        variance_reason_code: "COUNT_ERROR",
        reason: "Bin empty",
      }),
    );
    expect(await screen.findByText("No location below zero.")).toBeInTheDocument();
  });

  it("shows how a reconciled item was closed, by a count or regularised by receipt", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 2,
          status: "closed",
          closure_kind: "receipt",
          closed_at: TS,
          closing_movement_type: "GR_ACCEPT",
        }),
      ],
      double_counts: [],
      over_reservations: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    await waitFor(() => expect(mocks.listNegativeStockReviews).toHaveBeenLastCalledWith(true));
    expect(
      await screen.findByText(/Regularised by receipt on .* \(Goods receipt\)/),
    ).toBeInTheDocument();
  });

  it("names both counts of a double count and offers a new count", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [],
      double_counts: [
        {
          article_id: 21,
          article_code: "A-21",
          article_name: "Bearing",
          location_id: 10,
          location_code: "BIN",
          warehouse_code: "MAIN",
          on_hand_qty: 5,
          reserved_qty: 0,
          available_qty: 5,
          counts: [
            {
              count_sync_id: "s1",
              path: "reconciliation",
              system_qty: -5,
              session_code: "CC-1",
              counted_by: "Alice",
              device_code: "7KQ2",
              counted_at: TS,
              counted_qty: 0,
              variance_qty: 5,
            },
            {
              count_sync_id: "s2",
              path: "count_session",
              system_qty: -5,
              session_code: "CC-2",
              counted_by: "Bob",
              device_code: "M3XZ",
              counted_at: TS,
              counted_qty: 0,
              variance_qty: 5,
            },
          ],
        },
      ],
      over_reservations: [],
    });
    render(<NegativeStockReviewPanel />);
    expect(await screen.findByText("Double count — to verify")).toBeInTheDocument();
    expect(
      screen.getByText(/Alice on PC 7KQ2, .*, by stock-to-reconcile list: counted 0/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Bob on PC M3XZ, .*, by count session CC-2: counted 0/),
    ).toBeInTheDocument();
    expect(screen.queryByText("No location below zero.")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Record a count" }));
    expect(await screen.findByRole("button", { name: "Record the count" })).toBeDisabled();
  });

  it("labels a manual entry as an adjustment, not a receipt", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 3,
          status: "closed",
          closure_kind: "adjustment",
          closed_at: TS,
          closing_movement_type: "ADJUST_IN",
        }),
      ],
      double_counts: [],
      over_reservations: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(/Reconciled by an adjustment with its reason on .*\(Stock entry\)/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Regularised by receipt/)).not.toBeInTheDocument();
  });

  it("says when an entry made before the shortage was visible closed the item", async () => {
    mocks.listNegativeStockReviews.mockResolvedValue({
      items: [
        item({
          id: 4,
          status: "closed",
          closure_kind: "prior_entry",
          closed_at: TS,
          closing_movement_type: "ADJUST_IN",
        }),
      ],
      double_counts: [],
      over_reservations: [],
    });
    render(<NegativeStockReviewPanel />);
    fireEvent.click(await screen.findByLabelText("Show reconciled items"));
    expect(
      await screen.findByText(/Regularised by an earlier entry on .*no reason asked/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Reconciled by an adjustment/)).not.toBeInTheDocument();
    expect(frInventory.negativeStock.closure.prior_entry).toMatch(
      /^Régularisé par saisie antérieure/,
    );
  });

  it("has every text in French too", () => {
    const keys = (o: Record<string, unknown>, p = ""): string[] =>
      Object.entries(o).flatMap(([k, v]) =>
        v && typeof v === "object" ? keys(v as Record<string, unknown>, `${p}${k}.`) : [`${p}${k}`],
      );
    const en = keys({ negativeStock: enInventory.negativeStock, errors: enInventory.errors });
    const fr = keys({ negativeStock: frInventory.negativeStock, errors: frInventory.errors });
    expect(fr).toEqual(en);
    expect(frInventory.negativeStock.flag).toBe("Négatif — à rapprocher");
    expect(frInventory.negativeStock.closure.receipt).toMatch(/^Régularisé par réception/);
  });
});
